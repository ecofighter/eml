//! REPL のセッション。入力ごとにセッション全体を前端から検査し直し、新しい関数だけを、入力をまたいで持つ `Machine` の
//! `CodeBase` に足す (docs/spec/modules.md の「REPL のセッション」)。診断は各段階が作り、
//! ここは段階をつなぐだけである。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use eml_diagnostics::{Diagnostic, SourceFiles, TextSize, has_errors};
use eml_hir::{
    FunctionId, FunctionKind, ImportCache, InputForm, InputKind, ModuleSource, SessionInput,
};
use eml_types::{InputDisplay, InstanceNode, TypedProgram};
use eml_vm::{AnyMachine, Control, HostRef, RunConfig, RuntimeError, SourceLocation, VmFault};

use crate::{Session, pretty};

/// 結果を整形する既定の幅と、いちばん狭い幅 (docs/spec/repl.md の「結果の表示」)。
const DEFAULT_WIDTH: usize = 80;
const MIN_WIDTH: usize = 20;

/// REPL のセッション。端末は `eml repl` で、`terminal.rs` が使う。
pub struct Repl {
    control: Arc<Control>,
    source: Box<dyn ModuleSource + Send>,
    root: String,
    /// `:load` のファイルの表示のパス。ファイルを受け付けたかによらず持つ。
    file: Option<String>,
    machine: AnyMachine,
    /// 受け付けた定義と `let` の入力。式の入力は後の入力から参照されないので持たない。
    accepted: Vec<Accepted>,
    cache: ImportCache,
    /// 最後に受け付けた時点の一様な位置。
    uniform: BTreeMap<UniformKey, Positions>,
    /// 受け付けた入力までに読んだファイルのパス。セッション全体を入力ごとに検査し直すので、前の入力の警告を繰り返さない
    /// ために使う。
    shown: HashSet<String>,
    next_input: u32,
    /// 続けられない誤りの後は、ヒープを信用できないので実行しない。
    broken: bool,
    /// 結果を整形する幅。
    width: usize,
}

struct Accepted {
    number: u32,
    text: String,
    /// `:load` のファイルの入力だけが `Some` で、ファイルの表示のパスを持つ。
    path: Option<String>,
}

/// 式の入力の結果の書き方。`Repl::run` が、実行の返した文字列を整形して書く (docs/spec/repl.md の「結果の表示」)。
struct Shown {
    /// `Show` を持たない型なら ` : T`、持つ型なら空。
    suffix: String,
    /// 書けなかったときの誤りの位置。入力の式の先頭である。
    at: SourceLocation,
}

/// `evaluate` が読む入力の形。問い合わせは `type_of` が別に作る。
#[derive(Clone, Copy)]
enum Form {
    Typed,
    File,
}

#[derive(Debug)]
pub struct Evaluated {
    /// 誤りと、この入力で初めて読んだファイルの警告。
    pub diagnostics: Vec<Diagnostic>,
    /// `diagnostics` の位置を表示するためのファイル。番号は `eval` ごとに振り直す。
    pub files: SourceFiles,
    pub outcome: Outcome,
}

/// `:type` の結果。
#[derive(Debug)]
pub struct Typed {
    /// 誤りと、受け付けた入力までに読んでいないファイルの警告。
    pub diagnostics: Vec<Diagnostic>,
    pub files: SourceFiles,
    /// 誤りがなければ `Show a => a -> String` の形の型。
    pub ty: Option<String>,
}

impl Evaluated {
    fn without_files(outcome: Outcome) -> Evaluated {
        Evaluated {
            diagnostics: Vec::new(),
            files: SourceFiles::new(),
            outcome,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 空かコメントだけの入力。入力の番号も進めない。
    Empty,
    Accepted,
    /// 診断に誤りがある。セッションは変わらない。
    Rejected,
    /// 実行を始める前に割り込まれた。セッションは変わらない (docs/spec/repl.md の「割り込み」)。
    Interrupted,
    /// `exit` で終わった。実行時エラーと同じく受け入れず、`let` の束縛も作らない。中断で後始末が済んでいるので、
    /// 回復は要らない (docs/spec/repl.md の「割り込みと実行時エラー」)。
    Exited(i64),
    /// 続けられる実行時エラー。`let` の束縛は作らない。
    Failed(RuntimeError),
    /// 続けられない誤り (ヒープの誤り、内部の誤り、`Machine::load` の誤り)。後の `eval` も実行しない。
    Broken(RuntimeError),
}

#[derive(Debug)]
pub struct Closed {
    /// 検査モードで、根を手放した後に残った物体と、根を手放すときの誤り。速いモードでは誤りがなければ `None` である。
    pub leaks: Option<RuntimeError>,
}

impl Repl {
    pub fn new(
        config: &RunConfig,
        source: Box<dyn ModuleSource + Send>,
        root: &str,
        host: HostRef,
    ) -> Repl {
        Repl {
            control: Arc::clone(&config.control),
            source,
            root: root.to_string(),
            file: None,
            machine: AnyMachine::new(config, host),
            accepted: Vec::new(),
            cache: ImportCache::default(),
            uniform: BTreeMap::new(),
            shown: HashSet::new(),
            next_input: 1,
            broken: false,
            width: DEFAULT_WIDTH,
        }
    }

    /// ファイルを最初の入力とする新しいセッションを作り、ファイルを評価する。`path` は表示のパスで、import の根はその
    /// 最後の `/` までである。`source` はそのディレクトリを読む。ファイルを途中の入力として足す API は作らない。前の入力が
    /// import したモジュールとファイルが同じになる場合や、根とファイルのディレクトリが食い違う場合が出るためである
    /// (docs/spec/repl.md の「ファイルの読み込み」)。
    pub fn with_file(
        config: &RunConfig,
        path: &str,
        text: &str,
        source: Box<dyn ModuleSource + Send>,
        host: HostRef,
    ) -> (Repl, Evaluated) {
        let root = &path[..path.rfind('/').map_or(0, |at| at + 1)];
        let mut repl = Repl::new(config, source, root, host);
        repl.file = Some(path.to_string());
        let evaluated = repl.evaluate(text, Form::File);
        (repl, evaluated)
    }

    /// 結果を整形する幅を変える。20 より狭い幅は 20 にする。端末は列の数を渡す。
    pub fn set_width(&mut self, width: usize) {
        self.width = width.max(MIN_WIDTH);
    }

    pub fn eval(&mut self, text: &str) -> Evaluated {
        self.evaluate(text, Form::Typed)
    }

    /// `text` を `:type` の問い合わせとしてセッションに対して検査する。lowering も実行もせず、入力を受け付けず、番号も
    /// 進めない。番号は次の入力の番号を使う。問い合わせは `CodeBase` に入らないので、同じ番号の `input$N` と食い違わない。
    /// 続けられない誤りの後のセッションでも検査する。前端だけを使い、ヒープに触れないためである
    /// (docs/spec/repl.md の「型の表示」)。
    pub fn type_of(&self, text: &str) -> Typed {
        // 入力を待つ間に立った割り込みで止めないため、`eval` と同じく始めに下ろす
        self.control.clear();
        let mut inputs = self.session_inputs();
        inputs.push(SessionInput {
            number: self.next_input,
            text,
            form: InputForm::Query,
        });
        let (session, _) = Session::load_session(
            &inputs,
            &self.root,
            self.entry_file(),
            &*self.source,
            &self.cache,
        );
        let checked = session.check();
        let diagnostics = self.reported(checked.diagnostics, session.files());
        // 線形性、網羅性、E2012 は型の文字列を作った後で報告するので、診断で決める
        let ty = if has_errors(&diagnostics) {
            None
        } else {
            checked.typed.query
        };
        Typed {
            diagnostics,
            files: session.into_files(),
            ty,
        }
    }

    fn evaluate(&mut self, text: &str, form: Form) -> Evaluated {
        // 前の入力で立った止める理由のビットを下ろす。確保量の上限で止まると `MEMORY` のビットが残り、後の入力が
        // すべて止まるため (docs/spec/object-model.md の「実行の制御」)
        self.control.clear();
        if only_trivia(text) {
            return Evaluated::without_files(Outcome::Empty);
        }
        if self.broken {
            return Evaluated::without_files(Outcome::Broken(broken_session()));
        }
        // 受け付けなかった入力の番号も使い回さない。`CodeBase` にすでに入った `input$N` と同じ名前で、違うコードが
        // できるため。`let` の根の番号は入力の番号なので、根の番号も使い回さない
        let number = self.next_input;
        self.next_input += 1;
        let mut inputs = self.session_inputs();
        inputs.push(SessionInput {
            number,
            text,
            form: match form {
                Form::Typed => InputForm::Typed,
                Form::File => InputForm::File {
                    path: self.file.as_deref().expect("a file input has a path"),
                },
            },
        });
        let (session, read) = Session::load_session(
            &inputs,
            &self.root,
            self.entry_file(),
            &*self.source,
            &self.cache,
        );
        let checked = session.check();
        let mut diagnostics = self.reported(checked.diagnostics, session.files());
        let mut uniform = BTreeMap::new();
        if !has_errors(&diagnostics) {
            uniform = uniform_positions(&checked.program, &checked.typed);
            if let Some(name) = first_change(&self.uniform, &uniform) {
                diagnostics.push(eml_types::input_changes_uniform(
                    session.entry(),
                    TextSize::of(text),
                    name,
                ));
            }
        }
        // 入力の種類は HIR から読む。item の並びは入力の関数を持たない
        let input = checked.program.input();
        let kind = input.map(|id| match checked.program[id].kind {
            FunctionKind::Input(kind) => kind,
            _ => unreachable!("`Program::input` is an input function"),
        });
        // 止める語は lowering の後、`load` の前に1回だけ読む。VM は入口では安全点を通らないので、ここで止めないと、
        // 入口から最初の呼び出しまでの extern と `root_set` が動く (docs/spec/repl.md の「割り込み」)
        let program = match input {
            Some(entry) if !has_errors(&diagnostics) => Some((
                eml_core_ir::lower(&checked.program, &checked.typed, entry, session.files()),
                shown(&checked.program, &checked.typed, entry, session.files()),
            )),
            _ => None,
        };
        let outcome = if has_errors(&diagnostics) {
            Outcome::Rejected
        } else if self.control.stop_bits() & Control::INTERRUPT != 0 {
            Outcome::Interrupted
        } else if let Some((program, shown)) = program {
            self.run(&program, shown)
        } else {
            Outcome::Accepted
        };
        if outcome == Outcome::Accepted {
            self.cache.extend(read);
            self.shown
                .extend(session.files().paths().map(str::to_string));
            if kind == Some(InputKind::Expr) {
                // 式の入力のモジュールは次の入力にないので、その item を覚えておくと、変わったと誤って判断する
                let module = &checked.program.modules[checked.program.entry].name;
                uniform.retain(|key, _| &key.module != module);
            } else {
                self.accepted.push(Accepted {
                    number,
                    text: text.to_string(),
                    path: match form {
                        Form::Typed => None,
                        Form::File => self.file.clone(),
                    },
                });
            }
            // 標準ライブラリのモジュールは使う入力でだけ読み込むので、この入力が読まなかったモジュールの item は、前の
            // 位置のまま覚えておく (docs/spec/modules.md の「REPL のセッション」)
            for (key, positions) in std::mem::take(&mut self.uniform) {
                uniform.entry(key).or_insert(positions);
            }
            self.uniform = uniform;
        }
        Evaluated {
            diagnostics,
            files: session.into_files(),
            outcome,
        }
    }

    /// 根をすべて手放す。検査モードでは、その後に残った物体をリークとして返す。
    pub fn close(self) -> Closed {
        Closed {
            leaks: self.machine.close().err(),
        }
    }

    /// 受け付けた入力を、セッションを読み直す形にする。`:load` のファイルの入力は、ファイルとして読み直す。
    /// ファイルの入力を `<input 1>` として読み直すと、診断の位置、E1030、警告の絞り込みが変わるためである
    /// (docs/spec/modules.md の「REPL のセッション」)。
    fn session_inputs(&self) -> Vec<SessionInput<'_>> {
        self.accepted
            .iter()
            .map(|accepted| SessionInput {
                number: accepted.number,
                text: &accepted.text,
                form: match &accepted.path {
                    Some(path) => InputForm::File { path },
                    None => InputForm::Typed,
                },
            })
            .collect()
    }

    /// `:load` のファイルの名前。入口のファイルとして `load_session` に渡す。
    fn entry_file(&self) -> &str {
        self.file
            .as_deref()
            .map_or("", |path| &path[path.rfind('/').map_or(0, |at| at + 1)..])
    }

    /// 誤りはどのファイルのものも返す。警告は、前の入力を受け付けたときに一度返したので、この入力で初めて読んだファイルの
    /// ものだけを返す。
    fn reported(&self, diagnostics: Vec<Diagnostic>, files: &SourceFiles) -> Vec<Diagnostic> {
        diagnostics
            .into_iter()
            .filter(|d| d.is_error() || !self.shown.contains(files.path(d.primary.file)))
            .collect()
    }

    fn run(&mut self, program: &eml_lir::Program, shown: Option<Shown>) -> Outcome {
        let loaded = match self.machine.load(program) {
            Ok(loaded) => loaded,
            // コード生成の誤りは正しいプログラムでは起きない。`CodeBase` が途中まで書き換わっているかもしれないので、
            // 続けない
            Err(error) => {
                self.broken = true;
                return Outcome::Broken(eml_vm::compile_error(program, error));
            }
        };
        let result = match &shown {
            Some(_) => self
                .machine
                .run_shown(loaded.entry, pretty::LIMIT + 1)
                .map(|(finished, text)| (finished.exit, text)),
            None => self
                .machine
                .run(loaded.entry)
                .map(|finished| (finished.exit, None)),
        };
        match (result, shown) {
            (Ok((Some(code), _)), _) => Outcome::Exited(code),
            (Ok((None, Some(text))), Some(shown)) => {
                let line = pretty::format(&text, &shown.suffix, self.width);
                match self.machine.write_line(&line) {
                    Ok(()) => Outcome::Accepted,
                    // 式の入力はセッションを変えないので、書けなくても続けられる
                    Err(fault) => Outcome::Failed(RuntimeError::Fault {
                        fault: VmFault::Extern(fault),
                        function: "<repl>".to_string(),
                        at: Some(shown.at),
                    }),
                }
            }
            (Ok(_), _) => Outcome::Accepted,
            (Err(error), _) if error.is_broken() => {
                self.broken = true;
                Outcome::Broken(error)
            }
            (Err(error), _) => Outcome::Failed(error),
        }
    }
}

/// 式の入力の結果の書き方。値を書かない入力 (`Unit` の式、`let`、定義) は `None` である。
fn shown(
    program: &eml_hir::Program,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> Option<Shown> {
    let suffix = match &typed.inputs.get(entry)?.display {
        InputDisplay::Unit => return None,
        InputDisplay::Value => String::new(),
        InputDisplay::Typed(ty) => format!(" : {ty}"),
    };
    // `let` の束縛の入力はいつも `Unit` を記録する (docs/spec/types.md の「REPL の入力」)
    debug_assert!(matches!(
        program[entry].kind,
        FunctionKind::Input(InputKind::Expr)
    ));
    let file = program.file(entry.module);
    let body = program.body(entry).expect("an input has a body");
    let position = files.line_col(file, body.exprs[body.root].range.start());
    Some(Shown {
        suffix,
        at: SourceLocation {
            path: files.path(file).to_string(),
            line: position.line,
            column: position.column,
        },
    })
}

/// `RuntimeError` は写せないので、続けられない誤りの後の入力には、そのことを言う内部の誤りを返す。
fn broken_session() -> RuntimeError {
    RuntimeError::Fault {
        fault: VmFault::Internal("an earlier input broke the session"),
        function: "<repl>".to_string(),
        at: None,
    }
}

/// 空白とコメントだけの入力か。入力の番号を進めずに何もしないので、読み込みの前に調べる。
fn only_trivia(text: &str) -> bool {
    let file = SourceFiles::new().add("<input>", text);
    let (tokens, _) = eml_syntax::lex(file, text);
    tokens.iter().all(|token| token.kind.is_trivia())
}

/// 入力をまたいで同じ item を指す鍵。HIR の番号は `eval` ごとに振り直すので、モジュールの名前とモジュールの中の番号で
/// 指す。前の入力のテキストと import したモジュールのテキストは変わらないので、モジュールの中の番号も変わらない。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UniformKey {
    module: String,
    item: UniformItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UniformItem {
    Function(u32),
    Instance(u32),
    Tuple(usize),
}

#[derive(Debug, PartialEq, Eq)]
struct Positions {
    /// 診断に書く名前。
    name: String,
    uniform: BTreeSet<usize>,
}

/// すべての関数と instance の一様な位置。一様な位置のない item も入れる。前からある item に位置が増えたことを
/// 見分けるため。
fn uniform_positions(
    program: &eml_hir::Program,
    typed: &TypedProgram,
) -> BTreeMap<UniformKey, Positions> {
    let function_key = |id: eml_hir::FunctionId| UniformKey {
        module: program.modules[id.module].name.clone(),
        item: UniformItem::Function(u32::from(id.local.into_raw())),
    };
    let instance_key = |id: eml_hir::InstanceId| UniformKey {
        module: program.modules[id.module].name.clone(),
        item: UniformItem::Instance(u32::from(id.local.into_raw())),
    };
    let mut positions = BTreeMap::new();
    for (id, function) in program.functions() {
        positions.insert(
            function_key(id),
            Positions {
                name: function.name.clone(),
                uniform: BTreeSet::new(),
            },
        );
    }
    for (id, instance) in program.instances() {
        positions.insert(
            instance_key(id),
            Positions {
                name: format!(
                    "instance {} {}",
                    program[instance.class].name, program[instance.head].name
                ),
                uniform: BTreeSet::new(),
            },
        );
    }
    for (id, position) in typed.uniform.functions() {
        if let Some(entry) = positions.get_mut(&function_key(id)) {
            entry.uniform.insert(position);
        }
    }
    for (node, position) in typed.uniform.instances() {
        let (key, name) = match node {
            InstanceNode::Declared(id) => (instance_key(id), String::new()),
            InstanceNode::Tuple(count) => (
                UniformKey {
                    module: String::new(),
                    item: UniformItem::Tuple(count),
                },
                format!("the instances of {count}-tuples"),
            ),
        };
        positions
            .entry(key)
            .or_insert(Positions {
                name,
                uniform: BTreeSet::new(),
            })
            .uniform
            .insert(position);
    }
    positions
}

/// 前に受け付けた item のうち、一様な位置が変わった最初のものの名前。今の入力が読み込まなかった標準ライブラリの
/// モジュールの item は、今のプログラムにないので比べない。
fn first_change<'a>(
    before: &'a BTreeMap<UniformKey, Positions>,
    now: &BTreeMap<UniformKey, Positions>,
) -> Option<&'a str> {
    before
        .iter()
        .find(|(key, positions)| {
            now.get(*key)
                .is_some_and(|now| now.uniform != positions.uniform)
        })
        .map(|(_, positions)| positions.name.as_str())
}
