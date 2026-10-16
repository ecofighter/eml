use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use eml_cli::{Control, FsProvider, RunConfig, Session, default_limits, script_host};
use eml_diagnostics::{has_errors, render};

#[cfg(unix)]
mod sigint;
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
    /// Check a file and run it if there are no errors (`eml FILE` does the same)
    Run {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        file: PathBuf,
        /// Arguments for the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Start an interactive session
    Repl {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        /// A file to load first, as with `:load`
        file: Option<PathBuf>,
    },
}

/// サブコマンドの名前。最初の引数がこのどれでもなく `-` で始まらなければ、`run` のファイルとして読む。
const SUBCOMMANDS: [&str; 4] = ["check", "run", "repl", "help"];

/// 使い方の誤りに 2 を使うのは、clap が引数の誤りで返す値に合わせるため (docs/implementation/architecture.md)。
fn main() -> ExitCode {
    let cli = Cli::parse_from(with_run(std::env::args_os().collect()));
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
        Command::Repl { debug_heap, file } => terminal::run(debug_heap, file),
        Command::Run {
            debug_heap,
            file,
            args,
        } => {
            // 読み込みとコンパイルの間の Ctrl-C も、シグナルで殺されずにこのハンドラが受ける。そこで立てたビットは、
            // 実行の最初の安全点で読まれる
            let control = Arc::new(Control::new(default_limits()));
            #[cfg(unix)]
            sigint::install(Arc::clone(&control), sigint::Mode::Run);
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_control(control);
            // 警告がプログラムの出力の後に出ないように、実行の前に表示する。
            let compiled = session.compile();
            eprint!("{}", render(&compiled.diagnostics, session.files()));
            let Some(program) = compiled.program else {
                return ExitCode::from(1);
            };
            // 相対パスはカレントディレクトリを基準にする
            match eml_cli::execute(&program, &config, script_host(PathBuf::new(), args)) {
                Ok(_) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    // 130 は、シェルが SIGINT で終わった子に報告する値である
                    ExitCode::from(if error.interrupted() { 130 } else { 1 })
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

/// 入口が読めないことは、読み込みの段に入る前に使い方の誤りとして報告する。依存先が読めないことは E1026 である。
fn load(path: &Path) -> Option<Session> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let path = exact_spelling(path);
            let path = path.as_path();
            // 根は入口のファイルのディレクトリである
            let root = path.parent().unwrap_or(Path::new(""));
            let source = FsProvider::new(root);
            Some(Session::load(&path.display().to_string(), &text, &source))
        }
        Err(error) => {
            eprintln!("error: cannot read `{}`: {error}", path.display());
            None
        }
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
