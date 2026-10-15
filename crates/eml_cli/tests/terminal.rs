//! `eml repl` のプロセスのテスト (docs/spec/repl.md)。標準入力をパイプにするので、行は `rustyline` を通らずに読まれ、
//! プロンプトは書かれない。

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn repl_with(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .arg("repl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // 書き終えて標準入力を閉じると、REPL は EOF で終わる
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn repl(input: &str) -> Output {
    repl_with(&[], input.as_bytes())
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// パイプからの読みではプロンプトを書かないので、標準出力は値だけである。
#[test]
fn repl_prints_values_and_keeps_definitions() {
    let output = repl("1 + 2\ninc : Int -> Int\ninc x = x + 1\n\ninc 41\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "3\n42\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn repl_reports_errors_and_continues() {
    let output = repl("nope\n1 / 0\n1 + 1\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "2\n");
    let errors = stderr(&output);
    assert!(errors.contains("[E1001]"), "{errors}");
    assert!(
        errors.contains("runtime error: division by zero"),
        "{errors}"
    );
}

#[test]
fn quit_ends_the_session() {
    for quit in [":quit", ":q"] {
        let output = repl(&format!("{quit}\n1 + 1\n"));
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(stdout(&output), "");
    }
}

#[test]
fn repl_ends_at_eof_after_the_last_line() {
    let output = repl("1 + 1");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "2\n");
}

/// 実行時エラーの後の回復が残った物体を消すので、検査モードの `close` はリークを報告しない。
#[test]
fn debug_heap_reports_no_leaks() {
    let output = repl_with(&["--debug-heap"], b"let xs = [1, 2, 3]\nxs\n1 / 0\nxs\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "[1, 2, 3]\n[1, 2, 3]\n");
}

#[test]
fn command_errors_are_reported_and_nothing_is_evaluated() {
    let output = repl(":foo\n:quit now\n:\n1\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "1\n");
    let errors = stderr(&output);
    assert!(errors.contains("unknown command ':foo'"), "{errors}");
    assert!(errors.contains("':quit' takes no argument"), "{errors}");
    assert!(errors.contains("missing command name"), "{errors}");
}

#[test]
fn a_block_is_read_as_one_input() {
    let output = repl(":{\ninc : Int -> Int\n\ninc x = x + 1\n:}\ninc 1\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "2\n");
}

#[test]
fn a_block_left_open_at_eof_is_an_error() {
    let output = repl(":{\n1\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("missing ':}'"));
}

#[test]
fn help_lists_the_commands() {
    let output = repl(":help\n");
    assert_eq!(output.status.code(), Some(0));
    let help = stdout(&output);
    assert!(
        help.contains(":quit") && help.contains(":help") && help.contains(":{"),
        "{help}"
    );
}

/// CRLF の行末は、`\r` を除いて読む (docs/spec/repl.md の「行の編集」)。
#[test]
fn repl_reads_crlf_input() {
    let output = repl("inc : Int -> Int\r\ninc x = x + 1\r\n\r\ninc 1\r\n");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "2\n");
}

/// 読めない行は、panic せずに `close` を呼んで 1 で終わる。
#[test]
fn repl_ends_with_one_on_unreadable_input() {
    let output = repl_with(&[], b"1 + 1\n\xff\xfe\n2\n");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "2\n");
    assert!(
        stderr(&output).contains("error: cannot read input"),
        "{}",
        stderr(&output)
    );
}

/// 空行のない最後の定義も評価し、その診断を見せる。
#[test]
fn repl_evaluates_an_unfinished_input_at_eof() {
    let output = repl("bad : Int -> Int\nbad x = x + \"s\"");
    assert_eq!(output.status.code(), Some(0));
    assert!(stderr(&output).contains("[E2001]"), "{}", stderr(&output));
}

#[cfg(unix)]
mod interrupts {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    use crate::cli::{DEADLINE, kill_after_deadline, sigint};
    use crate::common::temp_project;

    /// 標準入力を開いたまま、入力を1つずつ送る `eml repl`。標準出力と標準エラーは別のスレッドで行ごとに読み続ける。
    /// パイプが詰まると子が書き込みの中で止まり、安全点に来ないためである。
    struct LiveRepl {
        child: Child,
        stdin: ChildStdin,
        out: Receiver<String>,
        err: Receiver<String>,
        deadline: Instant,
    }

    fn lines_of(stream: impl Read + Send + 'static) -> Receiver<String> {
        let (send, receive) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        receive
    }

    impl LiveRepl {
        /// 最初に `"ready"` を評価し、その表示を待つ。ハンドラを入れる前の SIGINT は既定の動作でプロセスを終わらせる
        /// ので、SIGINT は表示が出てから送る。
        fn start(dir: &Path, args: &[&str]) -> LiveRepl {
            let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
                .arg("repl")
                .args(args)
                .current_dir(dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdin = child.stdin.take().unwrap();
            let out = lines_of(child.stdout.take().unwrap());
            let err = lines_of(child.stderr.take().unwrap());
            let mut repl = LiveRepl {
                child,
                stdin,
                out,
                err,
                deadline: Instant::now() + DEADLINE,
            };
            repl.send("\"ready\"");
            repl.expect_out("\"ready\"");
            repl
        }

        fn send(&mut self, input: &str) {
            writeln!(self.stdin, "{input}").unwrap();
            self.stdin.flush().unwrap();
        }

        /// `line` が標準出力に出るまで待つ。
        fn expect_out(&mut self, line: &str) {
            loop {
                let left = self.deadline.saturating_duration_since(Instant::now());
                match self.out.recv_timeout(left) {
                    Ok(got) if got == line => return,
                    Ok(_) => {}
                    Err(_) => {
                        kill_after_deadline(&mut self.child, &format!("`{line}` did not arrive"))
                    }
                }
            }
        }

        /// 標準エラーに `interrupted` が出るまで、100 ms ごとに SIGINT を送る。続けて届いた SIGINT は1つにまとまる
        /// ことがあり、読みに入る直前に届いた SIGINT は読みを止めない (docs/spec/repl.md の「残る制限」) ので、
        /// 送り続ける。REPL は SIGINT で終わらないので、何回送ってもよい。最後に送った SIGINT が、次の入力の始めの
        /// `clear` の後に届くと次の入力を止めるので、出た後に少し待つ。
        fn interrupt_until_reported(&mut self) {
            loop {
                sigint(&self.child);
                match self.err.recv_timeout(Duration::from_millis(100)) {
                    Ok(line) if line == "interrupted" => break,
                    _ if Instant::now() >= self.deadline => {
                        kill_after_deadline(&mut self.child, "the input was not interrupted")
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        /// `:quit` を送り、終了コードを返す。
        fn quit(mut self) -> Option<i32> {
            self.send(":quit");
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status.code();
                }
                if Instant::now() >= self.deadline {
                    kill_after_deadline(&mut self.child, "the session did not end");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// 出力し続ける入力を SIGINT で止めた後も、セッションは続く。
    #[test]
    fn an_interrupt_stops_a_repl_input_and_the_session_continues() {
        let dir = temp_project("cli-repl-sigint-loop", &[]);
        let mut repl = LiveRepl::start(&dir, &[]);
        // 定義を受け付けた後で SIGINT を送るので、定義が取り消されることはない
        repl.send(
            "spin : Int -> <IO> Unit\nspin n =\n  println (show n)\n  spin (n + 1)\n\n\"defined\"",
        );
        repl.expect_out("\"defined\"");
        repl.send("spin 0");
        repl.interrupt_until_reported();
        repl.send("\"after\"");
        repl.expect_out("\"after\"");
        let code = repl.quit();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(code, Some(0));
    }

    /// 読みの中で待つ入力も SIGINT で止まり、セッションは続く。検査モードで流すので、`:quit` の 0 は、割り込んだ
    /// 後の `close` がリークを報告しないことも確かめる。
    #[test]
    fn an_interrupt_stops_a_repl_input_waiting_in_a_read() {
        let dir = temp_project("cli-repl-sigint-fifo", &[]);
        let fifo = dir.join("fifo");
        let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let mut repl = LiveRepl::start(&dir, &["--debug-heap"]);
        repl.send(
            "slurp : Unit -> <IO> String\nslurp () =\n  let f = Fs.open \"fifo\"\n  \
             let (f, text) = Fs.read_all f\n  Fs.close f\n  text\n\n\"defined\"",
        );
        repl.expect_out("\"defined\"");
        repl.send("slurp ()");
        // 子の `Fs.open` は書き手が開くまで待つ。読み手のいないパイプを書き込み用に非同期で開くと ENXIO なので、
        // 開けた時点で子は読みに進んでいる。何も書かずに開いたままにして、子を読みの中で待たせる
        let writer = loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&fifo)
            {
                Ok(writer) => break writer,
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    if Instant::now() >= repl.deadline {
                        kill_after_deadline(&mut repl.child, "the input did not open the pipe");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        repl.interrupt_until_reported();
        repl.send("\"after\"");
        repl.expect_out("\"after\"");
        let code = repl.quit();
        drop(writer);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(code, Some(0));
    }

    /// 入力を待つ間の SIGINT では終わらない。立った止める語は次の入力の始めに下りる。
    #[test]
    fn interrupts_while_waiting_do_not_end_the_repl() {
        let dir = temp_project("cli-repl-sigint-idle", &[]);
        let mut repl = LiveRepl::start(&dir, &[]);
        for _ in 0..3 {
            sigint(&repl.child);
            std::thread::sleep(Duration::from_millis(50));
        }
        repl.send("1 + 1");
        repl.expect_out("2");
        let code = repl.quit();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(code, Some(0));
    }
}
