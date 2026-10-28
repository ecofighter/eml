//! `eml repl` の繰り返し (docs/spec/repl.md)。ここは行を読み、結果を表示するだけで、行を入力にまとめるのは
//! `eml_cli::lines`、入力を評価するのは `eml_cli::Repl` である。

use std::fs;
use std::io::{self, BufRead, IsTerminal, StdinLock};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use eml_cli::lines::{Command, Lines, Step};
use eml_cli::{
    Clock, Control, Evaluated, FsProvider, HostRef, Outcome, Repl, RunConfig, Sched, repl_limits,
    stdio_host,
};
use eml_diagnostics::render;
use rustyline::error::ReadlineError;
use rustyline::{Behavior, Config, DefaultEditor};

const HELP: &str = "\
Commands:
  :quit          end the session
  :help          show this list
  :load <file>   start a new session from a file; without a file, an empty session
  :reload        load the last file again
  :type <expr>   show the type of an expression
  :{             start an input of several lines, blank lines included; a line `:}` ends it
A command may be shortened to any prefix of its name (`:q`, `:l`, `:r`, `:t`).";

pub fn run(debug_heap: bool, sched: Sched, clock: Clock, file: Option<PathBuf>) -> ExitCode {
    let control = Arc::new(Control::new(repl_limits()));
    crate::interrupt::install(Arc::clone(&control), crate::interrupt::Mode::Repl);
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_sched(sched)
        .with_clock(clock)
        .with_control(Arc::clone(&control));
    let file = match file {
        None => None,
        Some(path) => {
            let shown = crate::entry_name(&path);
            match fs::read_to_string(&shown) {
                Ok(text) => Some((shown, text)),
                Err(error) => {
                    // `eml run` の入口と同じく、読めないファイルは使い方の誤りにする
                    eprintln!("error: cannot read `{}`: {error}", path.display());
                    return ExitCode::from(2);
                }
            }
        }
    };
    // Host の相対パスは、`eml run` と同じくカレントディレクトリを基準にする
    let host = stdio_host(PathBuf::new());
    // 端末の大きさの変更は追わない (docs/spec/repl.md の「結果の表示」)
    let width = stdout_width();
    let repl = empty_session(&config, &host, width);
    let mut terminal = Terminal {
        config,
        control,
        host,
        width,
        repl,
        file: None,
        close_failed: false,
    };
    if let Some((shown, text)) = file
        && let Flow::Broken = terminal.load_text(&shown, &text)
    {
        return ExitCode::from(1);
    }
    let mut reader = match Reader::new() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("error: cannot read input: {error}");
            return terminal.close(ExitCode::from(1));
        }
    };
    let mut lines = Lines::new();
    loop {
        let flow = match reader.read(lines.prompt()) {
            Read::Line(text) => steps(&mut terminal, lines.push_text(&text)),
            Read::Interrupted => {
                lines.interrupt();
                Flow::Continue
            }
            Read::Eof => match steps(&mut terminal, lines.end()) {
                Flow::Continue | Flow::Interrupted => Flow::Quit,
                flow => flow,
            },
            Read::Failed(error) => {
                eprintln!("error: cannot read input: {error}");
                return terminal.close(ExitCode::from(1));
            }
        };
        match flow {
            Flow::Continue => {}
            // 同じ読みで届いた残りの行は、`steps` が捨てている
            Flow::Interrupted => lines.interrupt(),
            Flow::Quit => return terminal.close(ExitCode::SUCCESS),
            // 続けられない誤りの後はヒープを信用できないので、`close` を呼ばない (docs/spec/repl.md の「結果の表示」)
            Flow::Broken => return ExitCode::from(1),
        }
    }
}

/// import の根はカレントディレクトリである (docs/spec/repl.md の「コマンドライン」)。
fn empty_session(config: &RunConfig, host: &HostRef, width: usize) -> Repl {
    let mut repl = Repl::new(
        config,
        Box::new(FsProvider::new(Path::new(""))),
        host.clone(),
    );
    repl.set_width(width);
    repl
}

/// 標準出力の端末の列の数。端末でないか、読めないか、0 なら 80 である (docs/spec/repl.md の「結果の表示」)。
#[cfg(unix)]
fn stdout_width() -> usize {
    if !io::stdout().is_terminal() {
        return 80;
    }
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // TIOCGWINSZ は渡した `winsize` に書くだけで、ほかの前提を持たない。読めなければ -1 を返し、`size` は 0 のままである
    let read = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) };
    if read == 0 && size.ws_col > 0 {
        usize::from(size.ws_col)
    } else {
        80
    }
}

#[cfg(not(unix))]
fn stdout_width() -> usize {
    80
}

/// 端末のセッション。`:load` はセッションを作り直すので、作るのに要る `RunConfig` と `HostRef` を持つ
/// (docs/spec/repl.md の「ファイルの読み込み」)。
struct Terminal {
    config: RunConfig,
    control: Arc<Control>,
    host: HostRef,
    /// 結果を整形する幅。`:load` で作り直すセッションにも渡す。
    width: usize,
    repl: Repl,
    /// `:reload` で読み直すファイルの表示のパス。
    file: Option<String>,
    /// `:load` が閉じたセッションに誤りがあった。終わるときの終了コードを 1 にする。
    close_failed: bool,
}

impl Terminal {
    fn eval(&mut self, text: &str) -> Flow {
        show(self.repl.eval(text))
    }

    /// 読めなければ、セッションも覚えたファイルも変えない (docs/spec/repl.md の「ファイルの読み込み」)。
    fn load(&mut self, path: &str) -> Flow {
        let shown = crate::entry_name(Path::new(path));
        match fs::read_to_string(&shown) {
            Ok(text) => self.load_text(&shown, &text),
            Err(error) => {
                eprintln!("error: cannot read `{path}`: {error}");
                Flow::Continue
            }
        }
    }

    /// 結果によらずファイルを覚えるので、誤りを直してから `:reload` で読める。
    fn load_text(&mut self, shown: &str, text: &str) -> Flow {
        self.replace_with_empty();
        // import の根は、`eml run` の入口と同じくファイルのディレクトリである
        let dir = Path::new(shown)
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new(""));
        let (mut repl, evaluated) = Repl::with_file(
            &self.config,
            shown,
            text,
            Box::new(FsProvider::new(dir)),
            self.host.clone(),
        );
        repl.set_width(self.width);
        self.repl = repl;
        self.file = Some(shown.to_string());
        show(evaluated)
    }

    /// 古いセッションを先に閉じるのは、閉じる誤りがファイルの診断より前に出るようにするためである。
    fn replace_with_empty(&mut self) {
        let old = std::mem::replace(
            &mut self.repl,
            empty_session(&self.config, &self.host, self.width),
        );
        if let Some(error) = old.close().leaks {
            eprintln!("runtime error: {error}");
            self.close_failed = true;
        }
    }

    fn type_of(&mut self, expr: &str) -> Flow {
        let typed = self.repl.type_of(expr);
        eprint!("{}", render(&typed.diagnostics, &typed.files));
        // 前端の検査の間の Ctrl-C は、ほかの入力と同じく残りの行を捨てる。型は書かない
        // (docs/spec/repl.md の「割り込み」)
        if self.control.stop_bits() & Control::INTERRUPT != 0 {
            eprintln!("interrupted");
            return Flow::Interrupted;
        }
        if let Some(ty) = typed.ty {
            println!("{expr} : {ty}");
        }
        Flow::Continue
    }

    /// 根を手放して終わる。検査モードのリークと、`:load` が閉じたセッションの誤りは 1 で終わらせる
    /// (docs/spec/repl.md の「終わり方」)。
    fn close(self, code: ExitCode) -> ExitCode {
        match self.repl.close().leaks {
            None if self.close_failed => ExitCode::from(1),
            None => code,
            Some(error) => {
                eprintln!("runtime error: {error}");
                ExitCode::from(1)
            }
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
fn steps(terminal: &mut Terminal, steps: impl IntoIterator<Item = Step>) -> Flow {
    for step in steps {
        let flow = match step {
            Step::Read => Flow::Continue,
            Step::Eval(text) => terminal.eval(&text),
            Step::Command(Command::Quit) => Flow::Quit,
            Step::Command(Command::Help) => {
                println!("{HELP}");
                Flow::Continue
            }
            Step::Command(Command::Load(Some(path))) => terminal.load(&path),
            Step::Command(Command::Load(None)) => {
                terminal.replace_with_empty();
                terminal.file = None;
                Flow::Continue
            }
            Step::Command(Command::Reload) => match terminal.file.clone() {
                Some(path) => terminal.load(&path),
                None => {
                    eprintln!("no file to reload; use ':load'");
                    Flow::Continue
                }
            },
            Step::Command(Command::Type(expr)) => terminal.type_of(&expr),
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

/// 入力と `:load` の結果を表示する (docs/spec/repl.md の「結果の表示」)。
fn show(evaluated: Evaluated) -> Flow {
    eprint!("{}", render(&evaluated.diagnostics, &evaluated.files));
    match evaluated.outcome {
        Outcome::Empty | Outcome::Accepted | Outcome::Rejected => {}
        // 止まった所の関数の名前は利用者が書いた名前でないので、割り込みは場所を書かない (docs/spec/repl.md の
        // 「結果の表示」)
        Outcome::Interrupted => {
            eprintln!("interrupted");
            return Flow::Interrupted;
        }
        Outcome::Exited(code) => eprintln!("exited with code {code}"),
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
