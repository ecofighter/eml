//! REPL のトランスクリプトのテスト (docs/implementation/testing.md の「REPL のトランスクリプト」)。`tests/repl` の下の
//! トランスクリプトをそれぞれ別の `Repl` に流し、入力ごとの表示、診断、実行時エラーと、`close` の結果を
//! スナップショットにする。入力の終わりの規則は、端末と同じ形で行ごとに確かめる。

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eml_cli::lines::{Command, Lines, Step};
use eml_cli::{
    CaptureHost, Control, Evaluated, FsProvider, HostRef, InputStatus, Outcome, Repl, RunConfig,
    RuntimeError, default_limits, input_status,
};

use crate::common::for_each_file;

fn repl_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/repl")
        .canonicalize()
        .unwrap()
}

/// トランスクリプトの1つの入力。`lines` は印を除いた行で、入力を終える空行 (`.` だけの行) を含む。
#[derive(Debug, PartialEq)]
struct Input {
    lines: Vec<String>,
}

impl Input {
    /// 先頭の `count` 行を、端末が行ごとに足していく形でつないだテキスト。
    fn text(&self, count: usize) -> String {
        self.lines[..count].join("\n")
    }

    /// 1行目が `:` で始まる入力は、端末と同じく `eml_cli::lines` で読むコマンドである。
    fn is_command(&self) -> bool {
        self.lines[0].trim().starts_with(':')
    }

    /// `eval` に渡すテキスト。入力を終える空行は含めない (docs/spec/layout.md の「REPL の入力の終わり」)。
    fn submitted(&self) -> String {
        let end = match self.lines.last() {
            Some(last) if self.lines.len() > 1 && last.trim().is_empty() => self.lines.len() - 1,
            _ => self.lines.len(),
        };
        self.text(end)
    }

    /// スナップショットに書く入力の行。印はトランスクリプトと同じである。
    fn echo(&self) -> String {
        let mut out = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            let mark = if index == 0 { '>' } else { '.' };
            if line.is_empty() {
                writeln!(out, "{mark}").unwrap();
            } else {
                writeln!(out, "{mark} {line}").unwrap();
            }
        }
        out
    }
}

/// `> ` か `>` だけの行 (エディタが行末の空白を消すため)。
fn marker(line: &str, mark: char) -> Option<&str> {
    let rest = line.strip_prefix(mark)?;
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix(' ')
    }
}

/// 空の行と `#` の行は入力の外のコメントで、その後に続きの行を置けない。続きの行が離れた入力に付くのを防ぐため。
fn parse_transcript(text: &str) -> Vec<Input> {
    let mut inputs: Vec<Input> = Vec::new();
    let mut open = false;
    for (index, line) in text.lines().enumerate() {
        if let Some(first) = marker(line, '>') {
            inputs.push(Input {
                lines: vec![first.to_string()],
            });
            open = true;
        } else if let Some(rest) = marker(line, '.') {
            assert!(
                open,
                "line {}: a `.` line continues the input right above it",
                index + 1
            );
            inputs.last_mut().unwrap().lines.push(rest.to_string());
        } else {
            assert!(
                line.is_empty() || line.starts_with('#'),
                "line {}: a transcript line starts with `> `, `. ` or `#`, or is empty",
                index + 1
            );
            open = false;
        }
    }
    inputs
}

/// 置き場所の規則を確かめて、スナップショットの名前の接尾辞を決める。1つのファイルのテストは
/// `<分類>/<名前>.repl`、モジュールを import するテストは `<分類>/<名前>/main.repl` で、そのディレクトリが import の
/// 根になる。UI テストと同じく、最上位からの相対パスを名前にする (docs/implementation/testing.md の「UI テスト」)。
fn classify(path: &Path) -> String {
    let relative = path.strip_prefix(repl_root()).unwrap();
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parts.as_slice() {
        [category, file] => {
            assert!(
                file != "main.repl",
                "tests/repl/{category}/main.repl must be in a test directory: tests/repl/{category}/<name>/main.repl"
            );
            format!("{category}/{file}")
        }
        [category, name, file] if file == "main.repl" => format!("{category}/{name}"),
        _ => panic!(
            "{} must be tests/repl/<category>/<name>.repl or tests/repl/<category>/<name>/main.repl",
            relative.display()
        ),
    }
}

/// 最後の行より前までは終わっておらず、最後の行で終わる。端末は行ごとにこの判定で入力の終わりを決めるので、
/// 端末と同じ形で確かめる。
fn assert_boundaries(suffix: &str, input: &Input) {
    for count in 1..input.lines.len() {
        assert_eq!(
            input_status(&input.text(count)),
            InputStatus::Incomplete,
            "{suffix}: the input should continue after {:?}",
            input.text(count)
        );
    }
    assert_eq!(
        input_status(&input.text(input.lines.len())),
        InputStatus::Complete,
        "{suffix}: the input should end at {:?}",
        input.text(input.lines.len())
    );
}

/// 1回の流し方の結果。`inputs` は入力ごとの表示で、速いモードと比べる。`closes` は `:load` と `:reload` が閉じた
/// セッションの `close` の結果で、入力の番号と組にする。リークは検査モードだけが数えるので、`inputs` に入れない。
struct Transcript {
    inputs: Vec<String>,
    closes: Vec<(usize, Option<RuntimeError>)>,
    close: Option<RuntimeError>,
    broken: bool,
}

impl Transcript {
    fn render(&self) -> String {
        let mut out = String::new();
        for (index, input) in self.inputs.iter().enumerate() {
            out.push_str(input);
            for (_, close) in self.closes.iter().filter(|(at, _)| *at == index) {
                render_close(&mut out, "--- close (load) ---", close);
            }
        }
        render_close(&mut out, "--- close ---", &self.close);
        out
    }
}

fn render_close(out: &mut String, header: &str, close: &Option<RuntimeError>) {
    writeln!(out, "{header}").unwrap();
    match close {
        None => out.push_str("no leaks\n"),
        Some(error) => writeln!(out, "{error}").unwrap(),
    }
}

/// 流している間のセッション。`:load` はセッションを作り直すので、作るのに要る `RunConfig` と `HostRef` を持つ。
/// 端末の `Terminal` と同じ形である (docs/spec/repl.md の「ファイルの読み込み」)。
struct Runner<'a> {
    config: RunConfig,
    host: HostRef,
    root: &'a Path,
    /// `<名前>/main.repl` の形のトランスクリプトか。`:load` のファイルはトランスクリプトのディレクトリに置くので、
    /// ディレクトリの形でだけ使える。
    directory: bool,
    repl: Repl,
    /// `:reload` で読み直すファイルの、トランスクリプトのディレクトリからのパス。
    file: Option<String>,
    closes: Vec<(usize, Option<RuntimeError>)>,
    broken: bool,
}

impl Runner<'_> {
    fn empty_session(&self) -> Repl {
        Repl::new(
            &self.config,
            Box::new(FsProvider::new(self.root)),
            self.host.clone(),
        )
    }

    /// 古いセッションを閉じ、その結果を入力 `index` の位置に記録する。
    fn replace_with_empty(&mut self, index: usize) {
        let empty = self.empty_session();
        let old = std::mem::replace(&mut self.repl, empty);
        self.closes.push((index, old.close().leaks));
    }

    /// 読めなければ、セッションも覚えたファイルも変えない。
    fn load(&mut self, index: usize, path: &str, block: &mut String) {
        assert!(
            self.directory,
            "`:load {path}` needs a transcript of the form <category>/<name>/main.repl"
        );
        let text = match fs::read_to_string(self.root.join(path)) {
            Ok(text) => text,
            Err(error) => {
                writeln!(block, "error: cannot read `{path}`: {error}").unwrap();
                return;
            }
        };
        self.replace_with_empty(index);
        let dir = Path::new(path).parent().unwrap_or(Path::new(""));
        let (repl, evaluated) = Repl::with_file(
            &self.config,
            path,
            &text,
            Box::new(FsProvider::new(self.root.join(dir))),
            self.host.clone(),
        );
        self.repl = repl;
        self.file = Some(path.to_string());
        self.record(&evaluated, block);
    }

    fn command(&mut self, index: usize, input: &Input, block: &mut String) {
        assert_eq!(
            input.lines.len(),
            1,
            "a command is one line: {:?}",
            input.lines
        );
        match Lines::new().push(&input.lines[0]) {
            Step::Command(Command::Type(expr)) => {
                let typed = self.repl.type_of(&expr);
                block.push_str(&eml_test_support::full_with_paths(
                    &typed.files,
                    &typed.diagnostics,
                ));
                if let Some(ty) = typed.ty {
                    writeln!(block, "{expr} : {ty}").unwrap();
                }
            }
            Step::Command(Command::Load(Some(path))) => self.load(index, &path, block),
            Step::Command(Command::Load(None)) => {
                self.replace_with_empty(index);
                self.file = None;
            }
            Step::Command(Command::Reload) => match self.file.clone() {
                Some(path) => self.load(index, &path, block),
                None => writeln!(block, "no file to reload; use ':load'").unwrap(),
            },
            Step::Command(command @ (Command::Quit | Command::Help)) => {
                panic!("a transcript does not use {command:?}")
            }
            Step::Error(message) => writeln!(block, "{message}").unwrap(),
            step @ (Step::Read | Step::Eval(_)) => {
                panic!("{:?} is not a command: {step:?}", input.lines[0])
            }
        }
    }

    fn record(&mut self, evaluated: &Evaluated, block: &mut String) {
        block.push_str(&eml_test_support::full_with_paths(
            &evaluated.files,
            &evaluated.diagnostics,
        ));
        match &evaluated.outcome {
            Outcome::Failed(error) => writeln!(block, "runtime error: {error}").unwrap(),
            Outcome::Broken(error) => {
                self.broken = true;
                writeln!(block, "runtime error (the session is broken): {error}").unwrap();
            }
            Outcome::Interrupted => writeln!(block, "interrupted").unwrap(),
            Outcome::Exited(code) => writeln!(block, "exited with code {code}").unwrap(),
            Outcome::Empty | Outcome::Accepted | Outcome::Rejected => {}
        }
    }
}

/// `eml run` と同じ既定の上限で流す。止める理由のビットは `eval` が入力ごとに下ろすので、1つの `Control` を
/// トランスクリプト全体で使う。`directory` は `<名前>/main.repl` の形のトランスクリプトかである。
fn run(inputs: &[Input], root: &Path, directory: bool, debug_heap: bool) -> Transcript {
    // REPL は標準入力から入力を読むので、プログラムには標準入力を渡さない (docs/spec/repl.md)
    let host = CaptureHost::new(root.to_path_buf()).without_stdin();
    let out = host.output();
    let host = host.into_ref();
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(Arc::new(Control::new(default_limits())));
    let repl = Repl::new(&config, Box::new(FsProvider::new(root)), host.clone());
    let mut runner = Runner {
        config,
        host,
        root,
        directory,
        repl,
        file: None,
        closes: Vec::new(),
        broken: false,
    };
    let mut read = 0;
    let mut rendered = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let mut block = input.echo();
        let mut result = String::new();
        if input.is_command() {
            runner.command(index, input, &mut result);
        } else {
            let evaluated = runner.repl.eval(&input.submitted());
            runner.record(&evaluated, &mut result);
        }
        let all = out.contents();
        block.push_str(&all[read..]);
        read = all.len();
        block.push_str(&result);
        rendered.push(block);
    }
    Transcript {
        inputs: rendered,
        closes: runner.closes,
        close: runner.repl.close().leaks,
        broken: runner.broken,
    }
}

/// 検査モードの結果をスナップショットと比べた後で、速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` の
/// リークは検査モードだけが数えるので比べない。セッションを使えなくする誤りは検査モードだけが見つけ、速いモードでは
/// メモリを壊しうるので、そのトランスクリプトは速いモードで流さない (docs/implementation/testing.md の「2つのモードで
/// 流す」)。
#[test]
fn transcripts() {
    for_each_file(&repl_root(), "repl", false, |path, _| {
        let suffix = classify(path);
        let inputs = parse_transcript(&fs::read_to_string(path).unwrap());
        // コマンドの行は `input_status` でなく `eml_cli::lines` が読むので、入力の終わりの規則を当てない
        for input in inputs.iter().filter(|input| !input.is_command()) {
            assert_boundaries(&suffix, input);
        }
        let root = path.parent().expect("a transcript has a directory");
        let directory = path.file_name().is_some_and(|name| name == "main.repl");
        let checked = run(&inputs, root, directory, true);
        let mut settings = insta::Settings::clone_current();
        settings.set_input_file(path);
        settings.set_snapshot_suffix(&suffix);
        settings.bind(|| insta::assert_snapshot!(checked.render()));
        if !checked.broken {
            let fast = run(&inputs, root, directory, false);
            assert_eq!(
                fast.inputs, checked.inputs,
                "{suffix}: the fast mode printed or ended otherwise"
            );
        }
    });
}

#[test]
fn a_transcript_splits_into_inputs() {
    let inputs = parse_transcript("# note\n> f : Int -> Int\n. f x = x\n.\n\n> f 1\n>\n");
    assert_eq!(
        inputs,
        [
            Input {
                lines: vec!["f : Int -> Int".into(), "f x = x".into(), "".into()]
            },
            Input {
                lines: vec!["f 1".into()]
            },
            Input {
                lines: vec!["".into()]
            },
        ]
    );
    assert_eq!(inputs[0].submitted(), "f : Int -> Int\nf x = x");
    assert_eq!(inputs[0].text(3), "f : Int -> Int\nf x = x\n");
    assert_eq!(inputs[0].echo(), "> f : Int -> Int\n. f x = x\n.\n");
    assert_eq!(inputs[2].submitted(), "");
}

#[test]
#[should_panic(expected = "line 2: a `.` line continues the input right above it")]
fn a_continuation_after_a_comment_fails() {
    parse_transcript("# note\n. x\n");
}

#[test]
#[should_panic(expected = "line 1: a transcript line starts with")]
fn an_unmarked_line_fails() {
    parse_transcript("1 + 2\n");
}

#[test]
fn the_layout_decides_the_snapshot_name() {
    let root = repl_root();
    assert_eq!(
        classify(&root.join("basics/expressions.repl")),
        "basics/expressions.repl"
    );
    assert_eq!(
        classify(&root.join("modules/user_module/main.repl")),
        "modules/user_module"
    );
}

#[test]
#[should_panic(expected = "must be in a test directory")]
fn main_right_under_a_category_fails() {
    classify(&repl_root().join("modules/main.repl"));
}

#[test]
#[should_panic(expected = "must be tests/repl/<category>/<name>.repl")]
fn a_file_right_under_the_top_directory_fails() {
    classify(&repl_root().join("loose.repl"));
}
