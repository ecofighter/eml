//! 今の実装 (`eml_interp` と `eml_runtime`) が、`eml_rt` の extern の期待値の表と同じ結果になることを確かめる。
//! S10 で CEK を消すとき、このテストも消す (docs/implementation/testing.md の「期待値の表」)。

#[path = "../../eml_rt/src/heap/tests/table.rs"]
mod table;

use std::path::{Path, PathBuf};

use eml_interp::{RunConfig, RuntimeError, run};
use eml_runtime::OutputSink;
use table::{Arg, Case, Outcome};

/// 変数の番号を振りながら、プログラムの本体の行を積む
#[derive(Default)]
struct Body {
    lines: Vec<String>,
    next: u32,
}

impl Body {
    fn var(&mut self, name: &str) -> String {
        self.next += 1;
        format!("{name}.{}", self.next)
    }

    fn push(&mut self, line: String) {
        self.lines.push(format!("  {line}"));
    }

    fn label(&mut self, label: &str) {
        self.lines.push(format!("{label}:"));
    }

    /// 引数を1つ用意し、渡すアトムを返す。負の整数と `i64::MIN` は引き算で作る (テキストの形に依らないため)
    fn arg(&mut self, arg: &Arg, root: &Path, release: &mut Vec<String>) -> String {
        match arg {
            Arg::Int(v) if *v >= 0 => v.to_string(),
            Arg::Int(v) => {
                let x = self.var("n");
                let magnitude = v.unsigned_abs();
                if magnitude > i64::MAX as u64 {
                    let y = self.var("n");
                    self.push(format!(
                        "let {y}: int = extern \"Prelude.-\"(0, {})",
                        i64::MAX
                    ));
                    self.push(format!("let {x}: int = extern \"Prelude.-\"({y}, 1)"));
                } else {
                    self.push(format!(
                        "let {x}: int = extern \"Prelude.-\"(0, {magnitude})"
                    ));
                }
                x
            }
            Arg::Bool(b) => if *b { "#1" } else { "#0" }.to_string(),
            Arg::Const(s) => {
                let x = self.var("c");
                self.push(format!("let {x}: obj = const {s:?}"));
                x
            }
            Arg::Unique(s) => {
                let (c, e, x) = (self.var("c"), self.var("e"), self.var("u"));
                self.push(format!("let {c}: obj = const {s:?}"));
                self.push(format!("let {e}: obj = const \"\""));
                self.push(format!("let {x}: obj = extern \"Prelude.++\"({c}, {e})"));
                x
            }
            Arg::Shared(s) => {
                let x = self.arg(&Arg::Unique(s.clone()), root, release);
                self.push(format!("dup {x}"));
                release.push(x.clone());
                x
            }
            Arg::Rel(p) => self.arg(&Arg::Const(p.clone()), root, release),
            Arg::Abs(p) => {
                let path = root.join(p).to_str().unwrap().to_string();
                self.arg(&Arg::Const(path), root, release)
            }
        }
    }

    fn println(&mut self, s: &str) {
        let o = self.var("o");
        self.push(format!("let {o}: unit = extern \"Prelude.println\"({s})"));
    }

    /// `Shared` で残した参照を手放して終わる
    fn finish(&mut self, release: &[String]) {
        for x in release {
            self.push(format!("decref {x}"));
        }
        self.push("return ()".to_string());
    }

    /// タグで分かれ、タグの名前を出して終わる
    fn print_tag(&mut self, r: &str, layout: &str, names: &[&str], release: &[String]) {
        let cases: Vec<String> = (0..names.len())
            .map(|i| format!("#{i} -> b{}", i + 1))
            .collect();
        self.push(format!("switch {r} {layout} {{ {} }}", cases.join(", ")));
        for (i, name) in names.iter().enumerate() {
            self.label(&format!("b{}", i + 1));
            let c = self.var("c");
            self.push(format!("let {c}: obj = const {name:?}"));
            self.println(&c);
            self.finish(release);
        }
    }
}

/// 結果をどう出し、どう読み戻すか
#[derive(Clone, Copy)]
enum Shape {
    Int,
    Bool,
    Order,
    Str,
    Out,
}

fn shape(name: &str) -> Shape {
    match name {
        "Prelude.+" | "Prelude.-" | "Prelude.*" | "Prelude./" | "Prelude.%" | "Prelude.negate" => {
            Shape::Int
        }
        "Prelude.Ord Int.compare" | "Prelude.Ord String.compare" => Shape::Order,
        "Prelude.Show Int.show"
        | "Prelude.Show String.show"
        | "Prelude.++"
        | "Std.Fs.read_all"
        | "Std.Fs.read_all2" => Shape::Str,
        "Prelude.println" => Shape::Out,
        _ if name.starts_with("Prelude.Eq ") || name.starts_with("Prelude.Ord ") => Shape::Bool,
        _ => panic!("an extern the table does not know: {name}"),
    }
}

const LAYOUTS: &str = "\
layout Prelude.Bool { False, True }
layout Prelude.Ordering { LT, EQ, GT }
layout (,) { (,)(tobj, tobj) }
";

/// 1つの場合の LIR のプログラム
fn program(name: &str, args: &[Arg], root: &Path) -> String {
    let mut body = Body::default();
    let mut release = Vec::new();
    let atoms: Vec<String> = args
        .iter()
        .map(|a| body.arg(a, root, &mut release))
        .collect();
    let shape = shape(name);
    if name.starts_with("Std.Fs.read_all") {
        let (file, text) = read_all(&mut body, &atoms[0], name == "Std.Fs.read_all2");
        let u = body.var("u");
        body.push(format!("let {u}: unit = extern \"Std.Fs.close\"({file})"));
        body.println(&text);
        body.finish(&release);
    } else {
        let repr = match shape {
            Shape::Int => "int",
            Shape::Bool | Shape::Order => "enum",
            Shape::Str => "obj",
            Shape::Out => "unit",
        };
        let r = body.var("r");
        body.push(format!(
            "let {r}: {repr} = extern {name:?}({})",
            atoms.join(", ")
        ));
        match shape {
            Shape::Int => {
                let s = body.var("s");
                body.push(format!(
                    "let {s}: obj = extern \"Prelude.Show Int.show\"({r})"
                ));
                body.println(&s);
                body.finish(&release);
            }
            Shape::Str => {
                body.println(&r);
                body.finish(&release);
            }
            Shape::Out => body.finish(&release),
            Shape::Bool => body.print_tag(&r, "Prelude.Bool", &["False", "True"], &release),
            Shape::Order => body.print_tag(&r, "Prelude.Ordering", &["LT", "EQ", "GT"], &release),
        }
    }
    format!(
        "{LAYOUTS}fn main() -> unit {{\n{}\n}}\n",
        body.lines.join("\n")
    )
}

/// 開いて読む。`twice` なら同じ File をもう一度読み、1回目の文字列は手放す。File と出す文字列を返す
fn read_all(body: &mut Body, path: &str, twice: bool) -> (String, String) {
    let h = body.var("h");
    body.push(format!("let {h}: obj lin = extern \"Std.Fs.open\"({path})"));
    let mut file = h;
    let mut text: Option<String> = None;
    for _ in 0..if twice { 2 } else { 1 } {
        let (t, f, s) = (body.var("t"), body.var("f"), body.var("s"));
        body.push(format!(
            "let {t}: obj lin = extern \"Std.Fs.read_all\"({file})"
        ));
        body.push(format!("unpack {t} (,) #0({f}: obj lin, {s}: obj)"));
        body.push(format!("release {t} (,) #0({f}, {s})"));
        if let Some(old) = text.replace(s) {
            body.push(format!("decref {old}"));
        }
        file = f;
    }
    (file, text.unwrap())
}

/// 場合を1つ動かし、結果を表の形に戻す
fn outcome(name: &str, args: &[Arg], root: &Path) -> Outcome {
    let text = program(name, args, root);
    let program = eml_lir::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    eml_lir::verify(&program).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_file_root(root.to_path_buf());
    match run(&program, &config, &sink) {
        Ok(_) => {}
        Err(RuntimeError::Fault { fault, .. }) => return Outcome::Err(fault.to_string()),
        Err(error) => panic!("{error}\n{text}"),
    }
    let out = captured.contents();
    let printed = || {
        out.strip_suffix('\n')
            .unwrap_or_else(|| panic!("no line was printed: {out:?}\n{text}"))
            .to_string()
    };
    match shape(name) {
        Shape::Int => Outcome::Int(printed().parse().unwrap()),
        Shape::Bool => Outcome::Bool(match printed().as_str() {
            "True" => true,
            "False" => false,
            other => panic!("not a Bool: {other}"),
        }),
        Shape::Order => Outcome::Order(match printed().as_str() {
            "LT" => "LT",
            "EQ" => "EQ",
            "GT" => "GT",
            other => panic!("not an Ordering: {other}"),
        }),
        Shape::Str => Outcome::Str(printed()),
        Shape::Out => Outcome::Out(out),
    }
}

/// 作り直した一時ディレクトリ
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "eml_interp-rt-externs-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 表を作る。表を変えるときだけ、`cargo test -p eml_interp rt_externs -- --ignored` で流す
#[test]
#[ignore = "writes expected.txt"]
fn the_table_can_be_written_from_the_interpreter() {
    let root = scratch("write");
    table::fixtures(&root);
    let mut text =
        String::from("# eml_rt の extern の期待値の表。今の実装 (eml_interp) の出力から作った\n");
    for (name, args) in table::inputs() {
        let outcome = outcome(&name, &args, &root);
        text.push_str(&table::line(&Case {
            name,
            args,
            outcome,
        }));
        text.push('\n');
    }
    std::fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../eml_rt/src/heap/tests/expected.txt"),
        text,
    )
    .unwrap();
    std::fs::remove_dir_all(root).ok();
}

/// 今の実装が、表のどの場合も同じ結果になる。表の入力は `inputs` と同じで、古い表が残っていれば落ちる
#[test]
fn the_interpreter_matches_the_table() {
    let cases = table::cases();
    let inputs: Vec<(String, Vec<Arg>)> = cases
        .iter()
        .map(|c| (c.name.clone(), c.args.clone()))
        .collect();
    let expected = table::inputs();
    assert!(
        inputs == expected,
        "expected.txt is not written from the current inputs ({} cases, {} inputs)",
        inputs.len(),
        expected.len()
    );
    let root = scratch("check");
    let locked = table::fixtures(&root);
    for case in cases {
        if !locked
            && case
                .args
                .iter()
                .any(|a| matches!(a, Arg::Rel(p) if p == "locked.txt"))
        {
            continue;
        }
        assert_eq!(
            table::digest(outcome(&case.name, &case.args, &root)),
            case.outcome,
            "{}",
            table::line(&case)
        );
    }
    std::fs::remove_dir_all(root).ok();
}

/// `eml_lir` のタグと組の定数が、`eml_rt` の定数と同じである (S10 では VM の crate に移す)
#[test]
fn tags_agree_with_eml_lir() {
    assert_eq!(eml_lir::TRUE as i64, eml_rt::externs::TRUE);
    assert_eq!(eml_lir::FALSE as i64, eml_rt::externs::FALSE);
    assert_eq!(eml_lir::LT as i64, eml_rt::externs::LT);
    assert_eq!(eml_lir::EQ as i64, eml_rt::externs::EQ);
    assert_eq!(eml_lir::GT as i64, eml_rt::externs::GT);
    assert_eq!(eml_lir::TUPLE as u16, eml_rt::externs::TUPLE);
}

/// 書けない出力先への `println` は、`eml_rt` の `Fault::Output` と同じ文言で止まる
#[test]
fn a_failed_write_reads_the_same_on_both_sides() {
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let program = eml_lir::parse(
        "fn main() -> unit {\n  let s.1: obj = const \"x\"\n  let o.2: unit = extern Prelude.println(s.1)\n  return o.2\n}\n",
    )
    .unwrap();
    let Err(RuntimeError::Fault { fault, .. }) =
        run(&program, &RunConfig::default(), &OutputSink::new(Failing))
    else {
        panic!("println to a failing writer did not fail");
    };
    assert_eq!(
        fault.to_string(),
        eml_rt::Fault::Output("disk full".into()).to_string()
    );
}
