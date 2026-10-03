use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use eml_cli::{OutputSink, RunConfig, RunResult};
use eml_diagnostics::{FileId, SourceFiles, has_errors, render};

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
    /// Check a file and run it if there are no errors
    Run {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        file: PathBuf,
    },
}

/// 終了コード: 0 = 成功、1 = 診断のエラーか実行時エラー、2 = 使い方の誤り (clap が引数の誤りで 2 を返す)。
fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { file } => {
            let Some((files, id)) = load(&file) else {
                return ExitCode::from(2);
            };
            let diagnostics = eml_cli::check(&files, id);
            eprint!("{}", render(&diagnostics, &files));
            if has_errors(&diagnostics) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Command::Run { debug_heap, file } => {
            let Some((files, id)) = load(&file) else {
                return ExitCode::from(2);
            };
            let mut config = RunConfig::default();
            config.debug_heap = debug_heap;
            let outcome = eml_cli::run(&files, id, &config, OutputSink::stdout());
            eprint!("{}", render(&outcome.diagnostics, &files));
            match outcome.result {
                RunResult::Completed => ExitCode::SUCCESS,
                RunResult::NotRun => ExitCode::from(1),
                RunResult::RuntimeError(message) => {
                    eprintln!("runtime error: {message}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn load(path: &Path) -> Option<(SourceFiles, FileId)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let mut files = SourceFiles::new();
            let id = files.add(path.display().to_string(), text);
            Some((files, id))
        }
        Err(error) => {
            eprintln!("error: cannot read `{}`: {error}", path.display());
            None
        }
    }
}
