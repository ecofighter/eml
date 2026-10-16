//! `eml repl` の繰り返し (docs/spec/repl.md)。ここは行を読み、結果を表示するだけで、行を入力にまとめるのは
//! `eml_cli::lines`、入力を評価するのは `eml_cli::Repl` である。

use std::io::{self, BufRead, IsTerminal, StdinLock};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use eml_cli::lines::{Command, Lines, Step};
use eml_cli::{Control, FsProvider, Outcome, Repl, RunConfig, repl_limits, stdio_host};
use eml_diagnostics::render;
use rustyline::error::ReadlineError;
use rustyline::{Behavior, Config, DefaultEditor};

const HELP: &str = "\
Commands:
  :quit    end the session
  :help    show this list
  :{       start an input of several lines, blank lines included; a line `:}` ends it
A command may be shortened to any prefix of its name (`:q`).";

pub fn run(debug_heap: bool) -> ExitCode {
    let control = Arc::new(Control::new(repl_limits()));
    #[cfg(unix)]
    crate::sigint::install(Arc::clone(&control), crate::sigint::Mode::Repl);
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(control);
    // import の根と Host の相対パスは、`eml run` と同じくカレントディレクトリを基準にする
    let mut repl = Repl::new(
        &config,
        Box::new(FsProvider::new(Path::new(""))),
        "",
        stdio_host(PathBuf::new()),
    );
    let mut reader = match Reader::new() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("error: cannot read input: {error}");
            return close(repl, ExitCode::from(1));
        }
    };
    let mut lines = Lines::new();
    loop {
        let flow = match reader.read(lines.prompt()) {
            Read::Line(text) => steps(&mut repl, lines.push_text(&text)),
            Read::Interrupted => {
                lines.interrupt();
                Flow::Continue
            }
            Read::Eof => match steps(&mut repl, lines.end()) {
                Flow::Continue | Flow::Interrupted => Flow::Quit,
                flow => flow,
            },
            Read::Failed(error) => {
                eprintln!("error: cannot read input: {error}");
                return close(repl, ExitCode::from(1));
            }
        };
        match flow {
            Flow::Continue => {}
            // 同じ読みで届いた残りの行は、`steps` が捨てている
            Flow::Interrupted => lines.interrupt(),
            Flow::Quit => return close(repl, ExitCode::SUCCESS),
            // 続けられない誤りの後はヒープを信用できないので、`close` を呼ばない (docs/spec/repl.md の「結果の表示」)
            Flow::Broken => return ExitCode::from(1),
        }
    }
}

enum Flow {
    Continue,
    Quit,
    Broken,
    /// 入力を割り込みで止めた。
    Interrupted,
}

/// 結果を順に処理する。`:quit`、続けられない誤り、割り込みで止まれば、残りは捨てる。貼り付けた行を読んだ後の
/// Ctrl-C は、残りの行も止める (docs/spec/repl.md の「割り込み」)。
fn steps(repl: &mut Repl, steps: impl IntoIterator<Item = Step>) -> Flow {
    for step in steps {
        let flow = match step {
            Step::Read => Flow::Continue,
            Step::Eval(text) => eval(repl, &text),
            Step::Command(Command::Quit) => Flow::Quit,
            Step::Command(Command::Help) => {
                println!("{HELP}");
                Flow::Continue
            }
            // Task 6 で端末の動作に替える
            Step::Command(Command::Load(_) | Command::Reload | Command::Type(_)) => {
                eprintln!("this command is not available yet");
                Flow::Continue
            }
            Step::Error(message) => {
                eprintln!("{message}");
                Flow::Continue
            }
        };
        if !matches!(flow, Flow::Continue) {
            return flow;
        }
    }
    Flow::Continue
}

fn eval(repl: &mut Repl, text: &str) -> Flow {
    let evaluated = repl.eval(text);
    eprint!("{}", render(&evaluated.diagnostics, &evaluated.files));
    match evaluated.outcome {
        Outcome::Empty | Outcome::Accepted | Outcome::Rejected => {}
        // 止まった所の関数の名前は利用者が書いた名前でないので、割り込みは場所を書かない (docs/spec/repl.md の
        // 「結果の表示」)
        Outcome::Interrupted => {
            eprintln!("interrupted");
            return Flow::Interrupted;
        }
        Outcome::Failed(error) if error.interrupted() => {
            eprintln!("interrupted");
            return Flow::Interrupted;
        }
        Outcome::Failed(error) => eprintln!("runtime error: {error}"),
        Outcome::Broken(error) => {
            eprintln!("runtime error: {error}");
            return Flow::Broken;
        }
    }
    Flow::Continue
}

/// 根を手放して終わる。検査モードのリークは 1 で終わらせる。
fn close(repl: Repl, code: ExitCode) -> ExitCode {
    match repl.close().leaks {
        None => code,
        Some(error) => {
            eprintln!("runtime error: {error}");
            ExitCode::from(1)
        }
    }
}

enum Reader {
    Edited(Box<DefaultEditor>),
    Piped(StdinLock<'static>),
}

enum Read {
    Line(String),
    Interrupted,
    Eof,
    Failed(String),
}

impl Reader {
    /// 標準入力が端末でなければ `rustyline` を使わない。`TERM` が `dumb` などのとき、`rustyline` はパイプからの読み
    /// でもプロンプトを書くためである (docs/spec/repl.md の「行の編集」)。
    fn new() -> rustyline::Result<Reader> {
        if !io::stdin().is_terminal() {
            return Ok(Reader::Piped(io::stdin().lock()));
        }
        let config = Config::builder().behavior(Behavior::PreferTerm).build();
        Ok(Reader::Edited(Box::new(DefaultEditor::with_config(
            config,
        )?)))
    }

    fn read(&mut self, prompt: &str) -> Read {
        match self {
            // std の `read_line` は `EINTR` で読み直す。待つ間の SIGINT が立てた止める語は、次の入力の始めに下りる
            Reader::Piped(stdin) => {
                let mut line = String::new();
                match stdin.read_line(&mut line) {
                    Ok(0) => Read::Eof,
                    Ok(_) => {
                        if line.ends_with('\n') {
                            line.pop();
                        }
                        Read::Line(line)
                    }
                    Err(error) => Read::Failed(error.to_string()),
                }
            }
            Reader::Edited(editor) => match editor.readline(prompt) {
                Ok(line) => {
                    // 履歴に足せなくても、行は読めている
                    let _ = editor.add_history_entry(line.as_str());
                    Read::Line(line)
                }
                Err(ReadlineError::Eof) => Read::Eof,
                Err(error) if interrupted(&error) => Read::Interrupted,
                Err(error) => Read::Failed(error.to_string()),
            },
        }
    }
}

/// 編集中の Ctrl-C のほかに、`EINTR` で戻った誤りも Ctrl-C と同じに扱う。`rustyline` は端末を raw にする `tcsetattr`
/// を ISIG が立ったまま呼ぶので、その間の SIGINT で `EINTR` が返りうる。REPL は Ctrl-C で終わらない
/// (docs/spec/repl.md の「割り込み」)。
fn interrupted(error: &ReadlineError) -> bool {
    match error {
        ReadlineError::Interrupted => true,
        #[cfg(unix)]
        ReadlineError::Signal(rustyline::error::Signal::Interrupt) => true,
        ReadlineError::Io(error) => error.kind() == io::ErrorKind::Interrupted,
        #[cfg(unix)]
        ReadlineError::Errno(errno) => *errno as i32 == libc::EINTR,
        _ => false,
    }
}
