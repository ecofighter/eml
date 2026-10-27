use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser, Subcommand};
use eml_cli::{Control, FsProvider, RunConfig, Sched, Session, default_limits, script_host};
use eml_diagnostics::{has_errors, render};

mod interrupt;
mod terminal;

#[derive(Parser)]
#[command(name = "eml", version, about = "The eml programming language")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a file and print diagnostics
    Check { file: PathBuf },
    /// Check a file and run it if there are no errors (`eml FILE` does the same); the ARGS after FILE go to the script
    #[command(override_usage = "eml run [OPTIONS] <FILE> [ARGS]...")]
    Run {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        /// How tasks are scheduled: `normal` preempts on a timer, `sequential` never preempts, `seeded:N` preempts at points chosen by the seed N
        #[arg(
            long,
            value_name = "normal|sequential|seeded:N",
            default_value = "normal"
        )]
        sched: Sched,
        file: PathBuf,
    },
    /// Start an interactive session
    Repl {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        /// How tasks are scheduled: `normal` preempts on a timer, `sequential` never preempts, `seeded:N` preempts at points chosen by the seed N
        #[arg(
            long,
            value_name = "normal|sequential|seeded:N",
            default_value = "normal"
        )]
        sched: Sched,
        /// A file to load first, as with `:load`
        file: Option<PathBuf>,
    },
}

/// サブコマンドの名前。最初の引数がこのどれでもなく `-` で始まらなければ、`run` のファイルとして読む。
const SUBCOMMANDS: [&str; 4] = ["check", "run", "repl", "help"];

/// 使い方の誤りに 2 を使うのは、clap が引数の誤りで返す値に合わせるため (docs/implementation/architecture.md)。
fn main() -> ExitCode {
    let (argv, script_args) = split_script_args(with_run(std::env::args_os().collect()));
    let cli = Cli::parse_from(argv);
    match cli.command {
        Command::Check { file } => {
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let diagnostics = session.check().diagnostics;
            eprint!("{}", render(&diagnostics, session.files()));
            if has_errors(&diagnostics) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Command::Repl {
            debug_heap,
            sched,
            file,
        } => terminal::run(debug_heap, sched, file),
        Command::Run {
            debug_heap,
            sched,
            file,
        } => {
            // 読み込みとコンパイルの間の Ctrl-C も、OS の既定の動作でプロセスが終わらずに、この受け口が受ける。そこで立てたビットは、
            // 実行の最初の安全点で読まれる
            let control = Arc::new(Control::new(default_limits()));
            interrupt::install(Arc::clone(&control), interrupt::Mode::Run);
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_sched(sched)
                .with_control(control);
            // 警告がプログラムの出力の後に出ないように、実行の前に表示する。
            let compiled = session.compile();
            eprint!("{}", render(&compiled.diagnostics, session.files()));
            let Some(program) = compiled.program else {
                return ExitCode::from(1);
            };
            // 相対パスはカレントディレクトリを基準にする
            match eml_cli::execute(&program, &config, script_host(PathBuf::new(), script_args)) {
                // シェルと同じく、終了コードは下の 8 ビットである (docs/spec/stdlib.md の「`exit`」)
                Ok(finished) => ExitCode::from(finished.exit.map_or(0, |code| (code & 255) as u8)),
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    let code = if error.interrupted() {
                        interrupt::EXIT_CODE
                    } else {
                        1
                    };
                    ExitCode::from(code)
                }
            }
        }
    }
}

/// `eml FILE ARGS...` を `eml run FILE ARGS...` にする (docs/spec/stdlib.md の「起動」)。`-` で始まる最初の引数は今と同じく
/// clap が読むので、綴りを誤ったフラグは使い方の誤りのままである。
fn with_run(mut argv: Vec<OsString>) -> Vec<OsString> {
    let is_file = argv.get(1).is_some_and(|first| {
        let first = first.to_string_lossy();
        !first.starts_with('-') && !SUBCOMMANDS.contains(&first.as_ref())
    });
    if is_file {
        argv.insert(1, OsString::from("run"));
    }
    argv
}

/// `run` のファイルより後ろの引数を clap に渡さずに切り離す (docs/spec/stdlib.md の「起動」)。clap に読ませると、ファイルの直後の
/// `--debug-heap` や `--help` や `--` をスクリプトの引数にできない。ファイルは `run` の後の、値を取るフラグの値でない、`-` で
/// 始まらない最初の引数である。値を取るフラグと同じ引数は次の引数も取り、`--sched=seeded:7` のような引数は自分だけを取る。
fn split_script_args(mut argv: Vec<OsString>) -> (Vec<OsString>, Vec<OsString>) {
    if argv.get(1).is_none_or(|command| command != "run") {
        return (argv, Vec::new());
    }
    let value_flags = run_value_flags();
    let mut index = 2;
    while let Some(arg) = argv.get(index) {
        let arg = arg.to_string_lossy();
        if value_flags.iter().any(|flag| *flag == arg) {
            index += 2;
        } else if arg.starts_with('-') {
            index += 1;
        } else {
            let tail = argv.split_off(index + 1);
            return (argv, tail);
        }
    }
    (argv, Vec::new())
}

/// `run` の値を取るフラグの長い名前 (`--sched`)。clap の定義から読むので、フラグを足しても分け方の規則を直さずに済む。
fn run_value_flags() -> Vec<String> {
    let cli = Cli::command();
    let Some(run) = cli.find_subcommand("run") else {
        return Vec::new();
    };
    run.get_arguments()
        .filter(|arg| arg.get_action().takes_values())
        .filter_map(|arg| arg.get_long())
        .map(|long| format!("--{long}"))
        .collect()
}

/// 入口が読めないことは、読み込みの段に入る前に使い方の誤りとして報告する。依存先が読めないことは E1026 である。
fn load(path: &Path) -> Option<Session> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let shown = entry_name(path);
            // 根は入口のファイルのディレクトリである
            let root = Path::new(&shown).parent().unwrap_or(Path::new(""));
            let source = FsProvider::new(root);
            Some(Session::load(&shown, &text, &source))
        }
        Err(error) => {
            eprintln!("error: cannot read `{}`: {error}", path.display());
            None
        }
    }
}

/// 入口の表示のパス。モジュールの名前は `/` で区切るので、Windows では `\` を `/` に替える
/// (docs/spec/modules.md の「モジュール」)。
fn entry_name(path: &Path) -> String {
    let shown = exact_spelling(path).display().to_string();
    if cfg!(windows) {
        shown.replace('\\', "/")
    } else {
        shown
    }
}

/// 入口のファイル名を、ディレクトリの一覧にある綴りに直す。大文字小文字を区別しないファイルシステムでは
/// `t/server.em` で `t/Server.em` を開ける。渡された綴りのままでは、依存先の `import Server` が入口を指すと分からず、
/// 入口を別のモジュールとしてもう1回読んでしまう (docs/spec/modules.md の「モジュール」)。一覧を読めないか一致する
/// 名前がなければ、渡されたパスをそのまま使う。
fn exact_spelling(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.to_path_buf();
    };
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return path.to_path_buf();
    };
    let lowered = name.to_lowercase();
    let mut differently_cased = None;
    for entry in entries.flatten() {
        let Ok(actual) = entry.file_name().into_string() else {
            continue;
        };
        if actual == name {
            return path.to_path_buf();
        }
        if actual.to_lowercase() == lowered {
            differently_cased = Some(actual);
        }
    }
    match differently_cased {
        Some(actual) => path.with_file_name(actual),
        None => path.to_path_buf(),
    }
}
