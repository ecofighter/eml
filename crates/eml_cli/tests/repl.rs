//! REPL のトランスクリプトのテスト (docs/implementation/testing.md の「REPL のトランスクリプト」)。`tests/repl` の下の
//! トランスクリプトを1つずつ `Repl` に流し、入力ごとの表示、診断、実行時エラーと、`close` の結果をスナップショットに
//! する。入力の終わりの規則は、端末と同じ形で行ごとに確かめる。

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eml_cli::{
    Control, FsProvider, InputStatus, Outcome, Repl, RunConfig, RuntimeError, capture_host,
    default_limits, input_status,
};

use crate::common::glob_lock;

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

/// 1回の流し方の結果。`inputs` は入力ごとの表示で、速いモードと比べる。
struct Transcript {
    inputs: Vec<String>,
    close: Option<RuntimeError>,
    broken: bool,
}

impl Transcript {
    fn render(&self) -> String {
        let mut out = self.inputs.concat();
        out.push_str("--- close ---\n");
        match &self.close {
            None => out.push_str("no leaks\n"),
            Some(error) => writeln!(out, "{error}").unwrap(),
        }
        out
    }
}

/// `eml run` と同じ既定の上限で流す。止める理由のビットは `eval` が入力ごとに下ろすので、1つの `Control` を
/// トランスクリプト全体で使う。
fn run(inputs: &[Input], root: &Path, debug_heap: bool) -> Transcript {
    let (host, out) = capture_host(root.to_path_buf());
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(Arc::new(Control::new(default_limits())));
    let mut repl = Repl::new(&config, Box::new(FsProvider::new(root)), "", host);
    let mut read = 0;
    let mut rendered = Vec::new();
    let mut broken = false;
    for input in inputs {
        let evaluated = repl.eval(&input.submitted());
        let all = out.contents();
        let mut block = input.echo();
        block.push_str(&all[read..]);
        read = all.len();
        block.push_str(&eml_test_support::full_with_paths(
            &evaluated.files,
            &evaluated.diagnostics,
        ));
        match &evaluated.outcome {
            Outcome::Failed(error) => writeln!(block, "runtime error: {error}").unwrap(),
            Outcome::Broken(error) => {
                broken = true;
                writeln!(block, "runtime error (the session is broken): {error}").unwrap();
            }
            Outcome::Empty | Outcome::Accepted | Outcome::Rejected => {}
        }
        rendered.push(block);
    }
    Transcript {
        inputs: rendered,
        close: repl.close().leaks,
        broken,
    }
}

/// 検査モードの結果をスナップショットと比べた後で、速いモードでも流し、入力ごとの表示が同じことを確かめる。`close` の
/// リークは検査モードだけが数えるので比べない。セッションを使えなくする誤りは検査モードだけが見つけ、速いモードでは
/// メモリを壊しうるので、そのトランスクリプトは速いモードで流さない (docs/implementation/testing.md の「2つのモードで
/// 流す」)。
#[test]
fn transcripts() {
    let _glob = glob_lock();
    insta::glob!("repl", "**/*.repl", |path| {
        let suffix = classify(path);
        let inputs = parse_transcript(&fs::read_to_string(path).unwrap());
        for input in &inputs {
            assert_boundaries(&suffix, input);
        }
        let root = path.parent().expect("a transcript has a directory");
        let checked = run(&inputs, root, true);
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(&suffix);
        settings.bind(|| insta::assert_snapshot!(checked.render()));
        if !checked.broken {
            let fast = run(&inputs, root, false);
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
