use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser, Subcommand};
use eml_cli::{
    Clock, Control, Engine, Finished, FsProvider, Limits, LinkError, Linker, Masked, RunConfig,
    Sched, Session, default_limits, execute_on, script_host,
};
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
        /// Run in checked mode and print the run's counters on stderr after it finishes
        #[arg(long)]
        stats: bool,
        /// How many worker threads run `par` children: 1 or more (default: `EML_WORKERS`, else the number of available CPUs)
        #[arg(long, value_name = "N", value_parser = parse_workers)]
        workers: Option<usize>,
        /// Open VALUE in place of the library written as KEY in a `link` item (repeatable)
        #[arg(long = "link", value_name = "KEY=VALUE", value_parser = parse_link)]
        link: Vec<(String, String)>,
        /// The backend that `Std.Gui` opens windows with: `native`, or `stub`, which opens nothing and sends one close request to each window it shows (default: `EML_GUI`, else native)
        #[arg(long, value_name = "native|stub", value_parser = parse_gui)]
        gui: Option<GuiChoice>,
        /// Compile to machine code and run it on one worker, in place of the VM (not with `--debug-heap`, `--stats` or `--gui`)
        #[arg(long, conflicts_with_all = ["debug_heap", "stats", "gui"])]
        native: bool,
        file: PathBuf,
    },
    /// Start an interactive session
    Repl {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        /// Open VALUE in place of the library written as KEY in a `link` item (repeatable)
        #[arg(long = "link", value_name = "KEY=VALUE", value_parser = parse_link)]
        link: Vec<(String, String)>,
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
            link,
            file,
        } => terminal::run(debug_heap, link, file),
        Command::Run {
            debug_heap,
            stats,
            workers,
            link,
            gui,
            native,
            file,
        } => {
            // 環境変数の誤りも、ファイルを読む前に使い方の誤りとして断る
            let engine = if native { Engine::Native } else { Engine::Vm };
            if native && !eml_native::SUPPORTED {
                eprintln!("error: native code is not supported on this machine");
                return ExitCode::from(2);
            }
            let workers = match workers_for(engine, workers) {
                Ok(workers) => workers,
                Err(message) => {
                    eprintln!("error: {message}");
                    return ExitCode::from(2);
                }
            };
            #[cfg_attr(not(feature = "gui"), allow(unused_variables))]
            let gui = match gui_for(engine, gui) {
                Ok(gui) => gui,
                Err(message) => {
                    eprintln!("error: {message}");
                    return ExitCode::from(2);
                }
            };
            // 読み込みとコンパイルの間の Ctrl-C と `Terminate` のシグナルも、OS の既定の動作でプロセスが終わらずに、
            // このハンドラが受ける。そこで立てたビットは、実行の最初の安全点で読まれる
            let control = Arc::new(Control::new(Limits {
                workers,
                ..default_limits()
            }));
            let doorbell = interrupt::doorbell();
            interrupt::install(Arc::clone(&control), doorbell.clone(), interrupt::Mode::Run);
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let mut config = RunConfig::default()
                .with_debug_heap(debug_heap || stats)
                .with_sched(Sched::Normal)
                .with_clock(Clock::Real)
                .with_control(Arc::clone(&control));
            #[cfg(feature = "gui")]
            if engine == Engine::Vm {
                config = config.with_main_park(gui_park(gui, &control, doorbell.clone()));
            }
            if let Some(doorbell) = doorbell {
                config = config.with_doorbell(doorbell);
            }
            // 警告がプログラムの出力の後に出ないように、実行の前に表示する。
            let compiled = match engine {
                Engine::Vm => session.compile(),
                Engine::Native => session.compile_native(),
            };
            eprint!("{}", render(&compiled.diagnostics, session.files()));
            let Some(program) = compiled.program else {
                return ExitCode::from(1);
            };
            // Cのライブラリは、SIGINTとSIGTERMを塞いで開く。ライブラリの初期化が始めるスレッドは塞ぎを受け継ぎ、
            // 2つのシグナルはworker 0だけに届く。その間に来たシグナルは、このスレッドが塞ぎを外したときに受ける
            // (docs/spec/vm.md の「上限と割り込み」)
            let linked = {
                let _blocked = Masked::new();
                link_program(&program, &compiled.link_keys, link)
            };
            let linked = match linked {
                Ok(linked) => linked,
                Err(error) => {
                    eprintln!("link error: {error}");
                    return ExitCode::from(1);
                }
            };
            // 相対パスはカレントディレクトリを基準にする
            match execute_on(
                engine,
                &program,
                &linked,
                &config,
                script_host(PathBuf::new(), script_args),
            ) {
                // シェルと同じく、終了コードは下の 8 ビットである (docs/spec/stdlib.md の「`exit`」)
                Ok(finished) => {
                    if stats {
                        print_stats(&finished);
                    }
                    ExitCode::from(finished.exit.map_or(0, |code| (code & 255) as u8))
                }
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    let code = if error.interrupted() {
                        interrupt::INTERRUPT_EXIT_CODE
                    } else if error.terminated() {
                        interrupt::TERMINATE_EXIT_CODE
                    } else {
                        1
                    };
                    ExitCode::from(code)
                }
            }
        }
    }
}

/// 読み込む前に C の関数をつなぐ。どの `link` の item にも合わない KEY は、書き誤りとして断る
/// (docs/spec/vm.md の「`--link`」)。`Linker` はここで捨ててよい。開いたライブラリと作った橋渡しは、プロセスの間ずっと
/// 残る (docs/spec/vm.md の「ライブラリと記号」)。
fn link_program(
    program: &eml_lir::Program,
    link_keys: &BTreeSet<String>,
    overrides: Vec<(String, String)>,
) -> Result<eml_cli::Linked, LinkError> {
    if let Some((key, _)) = overrides.iter().find(|(key, _)| !link_keys.contains(key)) {
        return Err(LinkError::UnknownOverride { key: key.clone() });
    }
    Linker::new(overrides).link(&program.c_functions, &program.c_handles)
}

/// `--link` の値。KEY と VALUE はどちらも空にできない (docs/spec/stdlib.md の「起動」)。
fn parse_link(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.is_empty() && !value.is_empty() => {
            Ok((key.to_string(), value.to_string()))
        }
        _ => Err(format!("`{text}` is not KEY=VALUE")),
    }
}

/// worker の数の上限。run は worker ごとに止める語と欄を確保し、起こすときはすべての欄をたどるので、
/// 大きすぎる数は確保の失敗や遅い run でなく使い方の誤りにする (docs/spec/stdlib.md の「起動」)。
const MAX_WORKERS: usize = 1024;

/// `--workers` と `EML_WORKERS` の値。worker は1つ以上要るので、0 と数でない文字列は断る (docs/spec/stdlib.md の「起動」)。
fn parse_workers(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(0) => Err("the number of workers must be at least 1".to_string()),
        Ok(workers) if workers > MAX_WORKERS => Err(format!(
            "the number of workers must be at most {MAX_WORKERS}"
        )),
        Ok(workers) => Ok(workers),
        Err(_) => Err(format!("`{text}` is not a number of workers")),
    }
}

/// `--workers` がないときの数。`EML_WORKERS` を先に見て、なければ CPU の数にする。CPU の数を読めなければ 1 で、
/// 上限を超える CPU の数は上限に切る。
fn workers_from_env() -> Result<usize, String> {
    match std::env::var("EML_WORKERS") {
        Ok(text) => parse_workers(&text).map_err(|message| format!("EML_WORKERS: {message}")),
        Err(std::env::VarError::NotUnicode(_)) => Err("EML_WORKERS: not valid unicode".to_string()),
        Err(std::env::VarError::NotPresent) => Ok(std::thread::available_parallelism()
            .map_or(1, |cpus| usize::from(cpus).min(MAX_WORKERS))),
    }
}

/// `--gui`と`EML_GUI`の値。`Std.Gui`が窓を開くbackendを選ぶ (docs/spec/stdlib.mdの「起動」)。
#[derive(Clone, Copy)]
enum GuiChoice {
    Native,
    Stub,
}

fn parse_gui(text: &str) -> Result<GuiChoice, String> {
    match text {
        "native" => Ok(GuiChoice::Native),
        "stub" => Ok(GuiChoice::Stub),
        _ => Err(format!("`{text}` is not a GUI backend (native or stub)")),
    }
}

/// `eml run`のGUIの`Park`。OSの終了の操作は、シグナルと同じく実行を割り込む (docs/spec/vm.mdの「GUIのPark」)。
#[cfg(feature = "gui")]
fn gui_park(
    gui: GuiChoice,
    control: &Arc<Control>,
    doorbell: Option<Arc<eml_cli::Doorbell>>,
) -> eml_cli::ParkFactory {
    let backend = match gui {
        GuiChoice::Native => eml_cli::Backend::Native,
        GuiChoice::Stub => eml_cli::Backend::Stub(eml_cli::Script::default()),
    };
    let control = Arc::clone(control);
    eml_cli::gui_park(
        backend,
        Arc::new(move || {
            control.interrupt();
            if let Some(doorbell) = &doorbell {
                doorbell.ring();
            }
        }),
    )
}

/// workerの数。ネイティブのコードは1つのworkerで流れるので、1でない数は`--workers`でも`EML_WORKERS`でも使い方の誤りにする
/// (docs/spec/native.mdの「VMとの違い」)。
fn workers_for(engine: Engine, flag: Option<usize>) -> Result<usize, String> {
    if engine == Engine::Vm {
        return flag.map_or_else(workers_from_env, Ok);
    }
    match flag {
        Some(1) => Ok(1),
        Some(_) => Err("--native runs one worker; --workers must be 1".to_string()),
        None => match workers_from_env_if_set()? {
            None | Some(1) => Ok(1),
            Some(_) => Err("--native runs one worker; EML_WORKERS must be 1 or unset".to_string()),
        },
    }
}

/// `EML_WORKERS`があればその数を返す。ないときは`None`である。
fn workers_from_env_if_set() -> Result<Option<usize>, String> {
    match std::env::var("EML_WORKERS") {
        Ok(text) => parse_workers(&text)
            .map(Some)
            .map_err(|message| format!("EML_WORKERS: {message}")),
        Err(std::env::VarError::NotUnicode(_)) => Err("EML_WORKERS: not valid unicode".to_string()),
        Err(std::env::VarError::NotPresent) => Ok(None),
    }
}

/// GUIのbackend。ネイティブのコードは窓を開かないので、`EML_GUI`があれば使い方の誤りにする (`--gui`は`conflicts_with`で
/// 断る)。
fn gui_for(engine: Engine, flag: Option<GuiChoice>) -> Result<GuiChoice, String> {
    if engine == Engine::Vm {
        return flag.map_or_else(gui_from_env, Ok);
    }
    match std::env::var_os("EML_GUI") {
        Some(_) => Err("--native opens no windows; EML_GUI must be unset".to_string()),
        None => Ok(GuiChoice::Native),
    }
}

/// `--gui`がないときのbackend。`EML_GUI`がなければnativeにする。
fn gui_from_env() -> Result<GuiChoice, String> {
    match std::env::var("EML_GUI") {
        Ok(text) => parse_gui(&text).map_err(|message| format!("EML_GUI: {message}")),
        Err(std::env::VarError::NotUnicode(_)) => Err("EML_GUI: not valid unicode".to_string()),
        Err(std::env::VarError::NotPresent) => Ok(GuiChoice::Native),
    }
}

/// 実行時エラーでない run の終わりに、数えた仕事を標準エラーの1行に出す (docs/implementation/architecture.md の「CLI と lib API」)。
fn print_stats(finished: &Finished) {
    let fields: Vec<String> = finished
        .stats
        .fields()
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    eprintln!("stats: {}", fields.join(" "));
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
/// 始まらない最初の引数である。値を取るフラグと同じ引数は次の引数も取り、`--workers=2` のような引数は自分だけを取る。
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

/// `run` の値を取るフラグの長い名前 (`--workers`)。clap の定義から読むので、フラグを足しても分け方の規則を直さずに済む。
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

/// エントリが読めないことは、読み込みの段に入る前に使い方の誤りとして報告する。依存先が読めないことは E1026 である。
fn load(path: &Path) -> Option<Session> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let shown = entry_name(path);
            // 根はエントリのファイルのディレクトリである
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

/// エントリの表示のパス。モジュールの名前は `/` で区切るので、Windows では `\` を `/` に替える
/// (docs/spec/modules.md の「モジュール」)。
fn entry_name(path: &Path) -> String {
    let shown = exact_spelling(path).display().to_string();
    if cfg!(windows) {
        shown.replace('\\', "/")
    } else {
        shown
    }
}

/// エントリのファイル名を、ディレクトリの一覧にある綴りに直す。大文字小文字を区別しないファイルシステムでは
/// `t/server.em` で `t/Server.em` を開ける。渡された綴りのままでは、依存先の `import Server` がエントリを指すと分からず、
/// エントリを別のモジュールとしてもう1回読んでしまう (docs/spec/modules.md の「モジュール」)。一覧を読めないか一致する
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
