//! REPL のセッション。入力ごとにセッション全体を前端から検査し直し、新しい関数だけを、入力をまたいで持つ `Machine` の
//! `CodeBase` に足す (docs/spec/modules.md の「REPL のセッション」)。診断は各段階が作り、
//! ここは段階をつなぐだけである。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use eml_diagnostics::{Diagnostic, SourceFiles, TextSize, has_errors};
use eml_hir::{FunctionKind, ImportCache, InputKind, ModuleSource, SessionInput};
use eml_types::{InstanceNode, TypedProgram};
use eml_vm::{AnyMachine, Control, HostRef, RunConfig, RuntimeError, VmFault};

use crate::Session;

/// lib API だけで動く REPL のセッション。端末の REPL は S11c で足す。
pub struct Repl {
    control: Arc<Control>,
    source: Box<dyn ModuleSource + Send>,
    root: String,
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
}

struct Accepted {
    number: u32,
    text: String,
}

#[derive(Debug)]
pub struct Evaluated {
    /// 誤りと、この入力で初めて読んだファイルの警告。
    pub diagnostics: Vec<Diagnostic>,
    /// `diagnostics` の位置を表示するためのファイル。番号は `eval` ごとに振り直す。
    pub files: SourceFiles,
    pub outcome: Outcome,
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
            machine: AnyMachine::new(config, host),
            accepted: Vec::new(),
            cache: ImportCache::default(),
            uniform: BTreeMap::new(),
            shown: HashSet::new(),
            next_input: 1,
            broken: false,
        }
    }

    pub fn eval(&mut self, text: &str) -> Evaluated {
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
        let mut inputs: Vec<SessionInput> = self
            .accepted
            .iter()
            .map(|accepted| SessionInput {
                number: accepted.number,
                text: &accepted.text,
            })
            .collect();
        inputs.push(SessionInput { number, text });
        let (session, read) =
            Session::load_session(&inputs, &self.root, &*self.source, &self.cache);
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
        let outcome = match input {
            _ if has_errors(&diagnostics) => Outcome::Rejected,
            None => Outcome::Accepted,
            Some(entry) => {
                let program =
                    eml_core_ir::lower(&checked.program, &checked.typed, entry, session.files());
                self.run(&program)
            }
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
                });
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

    /// 誤りはどのファイルのものも返す。警告は、前の入力を受け付けたときに一度返したので、この入力で初めて読んだファイルの
    /// ものだけを返す。
    fn reported(&self, diagnostics: Vec<Diagnostic>, files: &SourceFiles) -> Vec<Diagnostic> {
        diagnostics
            .into_iter()
            .filter(|d| d.is_error() || !self.shown.contains(files.path(d.primary.file)))
            .collect()
    }

    fn run(&mut self, program: &eml_lir::Program) -> Outcome {
        let loaded = match self.machine.load(program) {
            Ok(loaded) => loaded,
            // コード生成の誤りは正しいプログラムでは起きない。`CodeBase` が途中まで書き換わっているかもしれないので、
            // 続けない
            Err(error) => {
                self.broken = true;
                return Outcome::Broken(eml_vm::compile_error(program, error));
            }
        };
        match self.machine.run(loaded.entry) {
            Ok(_) => Outcome::Accepted,
            Err(error) if error.is_broken() => {
                self.broken = true;
                Outcome::Broken(error)
            }
            Err(error) => Outcome::Failed(error),
        }
    }
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

/// 前に受け付けた item のうち、一様な位置が変わった最初のものの名前。
fn first_change<'a>(
    before: &'a BTreeMap<UniformKey, Positions>,
    now: &BTreeMap<UniformKey, Positions>,
) -> Option<&'a str> {
    before
        .iter()
        .find(|(key, positions)| now.get(*key).map(|now| &now.uniform) != Some(&positions.uniform))
        .map(|(_, positions)| positions.name.as_str())
}
