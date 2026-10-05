use std::cmp::Ordering;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, TUPLE, VarId,
};
use eml_runtime::{
    Closure, FileHandle, Frame, Heap, HeapError, ObjRef, OutputSink, Payload, Value,
};

/// 関数値の適用の結果。関数に入ったか、値ができたか (足りない引数のクロージャ)。
enum Applied {
    Entered,
    Value(Value),
}

/// 読み終えた呼び出し。環境を退避する前に引数を読むために、呼び出しを2段に分ける。
enum Prepared {
    Direct(FnIdx, Vec<Value>),
    Apply(Value, Vec<Value>),
    Handle {
        effect: u32,
        body: Value,
        clauses: Vec<Value>,
        ret: Option<Value>,
    },
    Perform {
        effect: u32,
        op: u32,
        args: Vec<Value>,
    },
    Resume {
        k: Value,
        arg: Value,
    },
}

/// 1つの命令を実行した後の状態。
enum Step {
    Continue,
    Finished,
}

/// 呼び出しから戻った後に再開するところと、呼び出しのフレームに退避する変数。
struct Resume<'p> {
    bind: VarId,
    resume: CExprId,
    saved: &'p [VarId],
}

/// 将来 `threads` などを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// `open` の相対パスの基準 (docs/spec/effects.md の「組み込みの `IO`」)。既定の空のパスはカレントディレクトリを指す。
    pub file_root: PathBuf,
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> Self {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_file_root(mut self, root: PathBuf) -> Self {
        self.file_root = root;
        self
    }
}

/// 実行時エラー (docs/spec/core-ir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// 実行中の関数で止まった。
    Fault { fault: Fault, function: String },
    /// `debug_heap` で、終了時に解放されていないオブジェクトがあった。記述子の名前ごとの数。
    Leak(Vec<(String, usize)>),
}

/// 実行中の関数で起きた誤り。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    DivisionByZero,
    IntegerOverflow,
    Heap(HeapError),
    Output(String),
    /// `open` がファイルを開けなかった。理由は `ErrorKind` から決めた固定の文言で、OS の文言は環境ごとに違うので使わない。
    FileOpen {
        path: String,
        reason: &'static str,
    },
    FileRead {
        path: String,
        reason: &'static str,
    },
    FileNotUtf8 {
        path: String,
    },
    /// 型検査と Core IR の変換が正しければ起きない誤り。
    Internal(&'static str),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::DivisionByZero => f.write_str("division by zero"),
            Fault::IntegerOverflow => f.write_str("integer overflow"),
            Fault::Heap(error) => write!(f, "{error}"),
            Fault::Output(error) => write!(f, "cannot write the output: {error}"),
            Fault::FileOpen { path, reason } => write!(f, "cannot open `{path}`: {reason}"),
            Fault::FileRead { path, reason } => write!(f, "cannot read `{path}`: {reason}"),
            Fault::FileNotUtf8 { path } => write!(f, "`{path}` is not valid UTF-8"),
            Fault::Internal(what) => write!(f, "internal error: {what}"),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::Fault { fault, function } => write!(f, "{fault} in `{function}`"),
            RuntimeError::Leak(live) => {
                let parts: Vec<String> =
                    live.iter().map(|(name, n)| format!("{n} {name}")).collect();
                write!(
                    f,
                    "memory leak: objects were not freed: {}",
                    parts.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

pub fn run(
    program: Arc<Program>,
    config: &RunConfig,
    out: &OutputSink,
) -> Result<(), RuntimeError> {
    let mut machine = Machine::new(&program, out, &config.file_root);
    machine.run()?;
    // 実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから
    if config.debug_heap {
        machine.check_leaks()?;
    }
    Ok(())
}

/// CEK 機械。制御 (`function` と `control`)、環境 (`slots`)、継続 (`cont`) からなる。
struct Machine<'p> {
    program: &'p Program,
    out: &'p OutputSink,
    file_root: &'p Path,
    heap: Heap,
    function: FnIdx,
    control: CExprId,
    /// 今の関数の環境。読み出しはスロットを書き換えない。参照の所有は Core IR の命令 (使用、`dup`、`decref`) が表し、
    /// verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
    slots: Vec<Option<Value>>,
    /// 継続の先頭のフレーム。最下部には常に `Frame::Io` がある。
    cont: ObjRef,
}

impl<'p> Machine<'p> {
    fn new(program: &'p Program, out: &'p OutputSink, file_root: &'p Path) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(Payload::Frame(Frame::Io));
        let entry = program.function(program.entry);
        Machine {
            program,
            out,
            file_root,
            heap,
            function: program.entry,
            control: entry.body,
            slots: vec![None; entry.vars.len()],
            cont,
        }
    }

    fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(Step::Finished) => return Ok(()),
                Ok(Step::Continue) => {}
                Err(fault) => {
                    let function = self.program.function(self.function).name.clone();
                    return Err(RuntimeError::Fault { fault, function });
                }
            }
        }
    }

    /// 1つの命令を実行する。
    fn step(&mut self) -> Result<Step, Fault> {
        let program = self.program;
        match program.function(self.function).expr(self.control) {
            CExpr::Let { var, rhs, body } => return self.bind(*var, rhs, *body),
            CExpr::Switch { scrutinee, arms } => {
                // scrutinee は move で受け取る。共有された値は `take_or_copy` がフィールドを複製して箱を手放すので、
                // どちらの場合も枝はフィールドの参照を1つずつ所有して始まる (docs/spec/core-ir.md)
                let (tag, fields) = match self.atom(scrutinee)? {
                    Value::Tag(tag) => (tag, Vec::new()),
                    Value::Obj(obj) => match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
                        Payload::Data { tag, fields } => (tag, fields),
                        _ => return Err(Fault::Internal("a switch on an object that is not data")),
                    },
                    _ => return Err(Fault::Internal("a switch on a value that is not a tag")),
                };
                let arm = arms
                    .iter()
                    .find(|arm| arm.tag == tag)
                    .ok_or(Fault::Internal("a switch without a matching arm"))?;
                if arm.fields.len() != fields.len() {
                    return Err(Fault::Internal(
                        "a switch arm binds a different number of fields than the value has",
                    ));
                }
                for (&field, value) in arm.fields.iter().zip(fields) {
                    self.slots[field.0 as usize] = Some(value);
                }
                self.control = arm.body;
            }
            CExpr::Return(atom) => {
                let value = self.atom(atom)?;
                return self.ret(value);
            }
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            CExpr::TailCall(call) => return self.call(call, None),
            CExpr::Join { scope, .. } => self.control = *scope,
            CExpr::Jump { join, args } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let values = self.atoms(args)?;
                let (params, body) = program.function(self.function).join(*join);
                for (param, value) in params.iter().zip(values) {
                    self.slots[param.0 as usize] = Some(value);
                }
                self.control = body;
            }
            CExpr::Dup { var, body } => {
                if let Value::Obj(obj) = self.read(*var)? {
                    self.heap.dup(obj).map_err(Fault::Heap)?;
                }
                self.control = *body;
            }
            CExpr::Decref { var, body } => {
                if let Value::Obj(obj) = self.read(*var)? {
                    self.heap.decref(obj).map_err(Fault::Heap)?;
                }
                self.control = *body;
            }
        }
        Ok(Step::Continue)
    }

    fn bind(&mut self, var: VarId, rhs: &'p Rhs, body: CExprId) -> Result<Step, Fault> {
        let value = match rhs {
            Rhs::Atom(atom) => self.atom(atom)?,
            Rhs::ConstString(index) => {
                let text = self.program.strings[*index as usize].clone();
                Value::Obj(self.heap.alloc(Payload::Str(text)))
            }
            Rhs::Prim(op, args) => {
                let args = self.atoms(args)?;
                self.prim(*op, &args)?
            }
            Rhs::Io(op, args) => {
                let args = self.atoms(args)?;
                self.io(*op, &args)?
            }
            // 所有している参照を1つ手放す。継続も RC が1のオブジェクトなので、これで解放される (docs/spec/core-ir.md)
            Rhs::Drop(atom) => {
                if let Value::Obj(obj) = self.atom(atom)? {
                    self.heap.decref(obj).map_err(Fault::Heap)?;
                }
                Value::Unit
            }
            Rhs::Call { call, saved } => {
                let resume = Resume {
                    bind: var,
                    resume: body,
                    saved,
                };
                return self.call(call, Some(resume));
            }
            Rhs::MakeClosure(function, args) => {
                let args = self.atoms(args)?;
                let closure = Closure {
                    function: function.0,
                    args,
                };
                Value::Obj(self.heap.alloc(Payload::Closure(closure)))
            }
            Rhs::Con { tag, args } => {
                let fields = self.atoms(args)?;
                Value::Obj(self.heap.alloc(Payload::Data { tag: *tag, fields }))
            }
        };
        self.slots[var.0 as usize] = Some(value);
        self.control = body;
        Ok(Step::Continue)
    }

    /// 呼び出す。引数は、環境を退避する前に読む。`resume` は、戻った値を受ける変数と再開する位置で、`None` なら
    /// フレームを積まない (末尾呼び出し)。
    fn call(&mut self, call: &Call, resume: Option<Resume<'p>>) -> Result<Step, Fault> {
        let prepared = match call {
            Call::Direct(callee, args) => Prepared::Direct(*callee, self.atoms(args)?),
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                Prepared::Apply(callee, self.atoms(args)?)
            }
            Call::Handle {
                effect,
                body,
                clauses,
                ret,
            } => Prepared::Handle {
                effect: *effect,
                body: self.atom(body)?,
                clauses: self.atoms(clauses)?,
                ret: ret.map(|ret| self.atom(&ret)).transpose()?,
            },
            Call::Perform { effect, op, args } => Prepared::Perform {
                effect: *effect,
                op: *op,
                args: self.atoms(args)?,
            },
            Call::Resume { k, arg } => Prepared::Resume {
                k: self.atom(k)?,
                arg: self.atom(arg)?,
            },
        };
        if let Some(resume) = resume {
            self.push_frame(resume)?;
        }
        match prepared {
            Prepared::Direct(callee, args) => {
                self.enter(callee, args);
                Ok(Step::Continue)
            }
            Prepared::Apply(callee, args) => self.apply_and_continue(callee, args),
            Prepared::Handle {
                effect,
                body,
                clauses,
                ret,
            } => {
                let frame = Frame::Handler {
                    effect,
                    clauses,
                    ret,
                    next: Some(self.cont),
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.apply_and_continue(body, vec![Value::Unit])
            }
            Prepared::Perform { effect, op, args } => self.perform(effect, op, args),
            Prepared::Resume { k, arg } => self.resume(k, arg),
        }
    }

    /// 関数値を適用する。関数に入らずに値ができたら (足りない引数のクロージャ)、その値を継続に返す。
    fn apply_and_continue(&mut self, callee: Value, args: Vec<Value>) -> Result<Step, Fault> {
        match self.apply(callee, args)? {
            Applied::Entered => Ok(Step::Continue),
            Applied::Value(value) => self.ret(value),
        }
    }

    /// 継続の連結リストを先頭から読み、同じエフェクトの一番内側の handler フレームを探す (docs/spec/core-ir.md)。
    fn find_handler(&self, effect: u32) -> Result<ObjRef, Fault> {
        let mut current = self.cont;
        loop {
            let Payload::Frame(frame) = self.heap.get(current).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            current = match frame {
                Frame::Handler { effect: other, .. } if *other == effect => return Ok(current),
                Frame::Handler { next, .. } => {
                    next.ok_or(Fault::Internal("a detached handler is in the continuation"))?
                }
                Frame::Return { next, .. } | Frame::Apply { next, .. } => *next,
                Frame::Io => return Err(Fault::Internal("an operation without a handler")),
            };
        }
    }

    /// handler フレームの外側を切り離して機械の継続に戻し、節を呼ぶ。先頭から handler フレームまでの区間が継続で、
    /// `once` の操作はそれを継続オブジェクトにして `k` として渡す。`never` の操作は再開しないので、区間をここで
    /// 解放する。区間のフレームが退避した値も、子をたどる解放で1回ずつ解放される (docs/spec/core-ir.md)。
    fn perform(&mut self, effect: u32, op: u32, mut args: Vec<Value>) -> Result<Step, Fault> {
        let handler = self.find_handler(effect)?;
        let Payload::Frame(Frame::Handler { clauses, next, .. }) =
            self.heap.get_mut(handler).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("a handler that is not a handler frame"));
        };
        let clause = *clauses
            .get(op as usize)
            .ok_or(Fault::Internal("an operation without a clause"))?;
        let outside = next
            .take()
            .ok_or(Fault::Internal("performing through a detached handler"))?;
        // 節のクロージャは handler フレームにも残るので、呼ぶ分の参照を足す
        if let Value::Obj(obj) = clause {
            self.heap.dup(obj).map_err(Fault::Heap)?;
        }
        let top = std::mem::replace(&mut self.cont, outside);
        let resumable = self
            .program
            .effects
            .get(effect as usize)
            .and_then(|info| info.operations.get(op as usize))
            .ok_or(Fault::Internal("an unknown operation"))?
            .resumable;
        if resumable {
            let k = self.heap.alloc(Payload::Continuation { top, handler });
            args.push(Value::Obj(k));
        } else {
            self.heap.decref(top).map_err(Fault::Heap)?;
        }
        self.apply_and_continue(clause, args)
    }

    /// 継続オブジェクトの handler フレームの外側に今の継続をつなぎ、先頭のフレームに値を返す。末尾でない `resume`
    /// では、その前に呼び出しのフレームが積まれている。`multi` の継続をもう一度使うなら継続は共有されていて、
    /// `take_or_copy` が区間を写す。どちらの場合も区間のフレームは一意なので、handler フレームを書き換えてよい
    /// (docs/spec/core-ir.md)。
    fn resume(&mut self, k: Value, value: Value) -> Result<Step, Fault> {
        let Value::Obj(obj) = k else {
            return Err(Fault::Internal(
                "resuming a value that is not a continuation",
            ));
        };
        let Payload::Continuation { top, handler } =
            self.heap.take_or_copy(obj).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal(
                "resuming an object that is not a continuation",
            ));
        };
        let current = self.cont;
        match self.heap.get_mut(handler).map_err(Fault::Heap)? {
            Payload::Frame(Frame::Handler { next, .. }) => *next = Some(current),
            _ => {
                return Err(Fault::Internal(
                    "a continuation whose handler is not a handler frame",
                ));
            }
        }
        self.cont = top;
        self.ret(value)
    }

    fn enter(&mut self, callee: FnIdx, args: Vec<Value>) {
        let target = self.program.function(callee);
        let mut slots = vec![None; target.vars.len()];
        for (param, value) in target.params.iter().zip(args) {
            slots[param.0 as usize] = Some(value);
        }
        self.slots = slots;
        self.function = callee;
        self.control = target.body;
    }

    /// 関数値を引数に適用する (docs/spec/core-ir.md の eval/apply)。引数の個数が揃えば関数に入り、足りなければ
    /// 引数を足したクロージャを値にし、余れば余りを持つフレームを積んでから関数に入る。
    fn apply(&mut self, callee: Value, mut args: Vec<Value>) -> Result<Applied, Fault> {
        let Value::Obj(obj) = callee else {
            return Err(Fault::Internal("applying a value that is not a closure"));
        };
        let closure = self.take_closure(obj)?;
        let function = FnIdx(closure.function);
        let mut all = closure.args;
        all.append(&mut args);
        let arity = self.program.function(function).params.len();
        match all.len().cmp(&arity) {
            Ordering::Equal => {
                self.enter(function, all);
                Ok(Applied::Entered)
            }
            Ordering::Less => {
                let closure = Closure {
                    function: closure.function,
                    args: all,
                };
                let value = self.heap.alloc(Payload::Closure(closure));
                Ok(Applied::Value(Value::Obj(value)))
            }
            Ordering::Greater => {
                let rest = all.split_off(arity);
                let frame = Frame::Apply {
                    args: rest,
                    next: self.cont,
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.enter(function, all);
                Ok(Applied::Entered)
            }
        }
    }

    /// 呼び出しはクロージャの所有権を受け取る。共有されていれば、ランタイムが中身を写して子の参照を数え直す
    /// (docs/spec/runtime.md)。
    fn take_closure(&mut self, obj: ObjRef) -> Result<Closure, Fault> {
        match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
            Payload::Closure(closure) => Ok(closure),
            _ => Err(Fault::Internal("applying an object that is not a closure")),
        }
    }

    /// 呼び出しの後で使う変数だけをフレームに退避する。フレームは、ちょうど所有している参照だけを持つ
    /// (docs/spec/core-ir.md)。
    fn push_frame(&mut self, resume: Resume<'p>) -> Result<(), Fault> {
        let saved = resume
            .saved
            .iter()
            .map(|&var| Ok((var.0, self.read(var)?)))
            .collect::<Result<Vec<_>, Fault>>()?;
        let frame = Frame::Return {
            function: self.function.0,
            resume: resume.resume.0,
            bind: resume.bind.0,
            saved,
            next: self.cont,
        };
        self.cont = self.heap.alloc(Payload::Frame(frame));
        Ok(())
    }

    /// 継続の先頭のフレームに値を返す。最下部の `Frame::Io` に届いたら、プログラムが終わる。
    /// 余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    fn ret(&mut self, mut value: Value) -> Result<Step, Fault> {
        loop {
            // フレームはつねに一意である。共有されうるのは継続オブジェクトだけで、再開するときに区間を写す (docs/spec/runtime.md)
            let Payload::Frame(frame) = self.heap.take(self.cont).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            match frame {
                Frame::Apply { args, next } => {
                    self.cont = next;
                    match self.apply(value, args)? {
                        Applied::Entered => return Ok(Step::Continue),
                        Applied::Value(result) => value = result,
                    }
                }
                Frame::Return {
                    function,
                    resume,
                    bind,
                    saved,
                    next,
                } => {
                    let function = FnIdx(function);
                    let mut slots = vec![None; self.program.function(function).vars.len()];
                    for (var, saved) in saved {
                        slots[var as usize] = Some(saved);
                    }
                    slots[bind as usize] = Some(value);
                    self.slots = slots;
                    self.function = function;
                    self.control = CExprId(resume);
                    self.cont = next;
                    return Ok(Step::Continue);
                }
                // 本体が値を返したので handler を外す。節のクロージャはもう呼ばない
                Frame::Handler {
                    clauses,
                    ret: on_return,
                    next,
                    ..
                } => {
                    for clause in clauses {
                        if let Value::Obj(obj) = clause {
                            self.heap.decref(obj).map_err(Fault::Heap)?;
                        }
                    }
                    self.cont =
                        next.ok_or(Fault::Internal("a detached handler received a value"))?;
                    if let Some(on_return) = on_return {
                        match self.apply(on_return, vec![value])? {
                            Applied::Entered => return Ok(Step::Continue),
                            Applied::Value(result) => value = result,
                        }
                    }
                }
                Frame::Io => {
                    if let Value::Obj(obj) = value {
                        self.heap.decref(obj).map_err(Fault::Heap)?;
                    }
                    return Ok(Step::Finished);
                }
            }
        }
    }

    /// 変数の値。読み出しはスロットを書き換えない。ヒープの値の所有権を渡すかどうかは Core IR の命令が決める
    /// (docs/spec/core-ir.md)。
    fn read(&self, var: VarId) -> Result<Value, Fault> {
        self.slots[var.0 as usize].ok_or(Fault::Internal("a variable read before it was bound"))
    }

    fn atom(&self, atom: &Atom) -> Result<Value, Fault> {
        Ok(match *atom {
            Atom::Var(var) => self.read(var)?,
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
        })
    }

    fn atoms(&self, atoms: &[Atom]) -> Result<Vec<Value>, Fault> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    fn prim(&mut self, op: PrimOp, args: &[Value]) -> Result<Value, Fault> {
        let int = |index: usize| match args[index] {
            Value::Int(n) => Ok(n),
            _ => Err(Fault::Internal(
                "an integer operation on a value that is not an integer",
            )),
        };
        let overflow = || Fault::IntegerOverflow;
        let tag = |b: bool| Value::Tag(if b { TRUE } else { FALSE });
        Ok(match op {
            PrimOp::IntAdd => Value::Int(int(0)?.checked_add(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntSub => Value::Int(int(0)?.checked_sub(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntMul => Value::Int(int(0)?.checked_mul(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntDiv | PrimOp::IntMod => {
                let (a, b) = (int(0)?, int(1)?);
                if b == 0 {
                    return Err(Fault::DivisionByZero);
                }
                // Rust と同じく 0 の方向に切り捨てる (docs/spec/declarations.md)
                let result = if op == PrimOp::IntDiv {
                    a.checked_div(b)
                } else {
                    a.checked_rem(b)
                };
                Value::Int(result.ok_or_else(overflow)?)
            }
            PrimOp::IntNeg => Value::Int(int(0)?.checked_neg().ok_or_else(overflow)?),
            PrimOp::IntEq => tag(int(0)? == int(1)?),
            PrimOp::IntNe => tag(int(0)? != int(1)?),
            PrimOp::IntLt => tag(int(0)? < int(1)?),
            PrimOp::IntLe => tag(int(0)? <= int(1)?),
            PrimOp::IntGt => tag(int(0)? > int(1)?),
            PrimOp::IntGe => tag(int(0)? >= int(1)?),
            PrimOp::Not => match args[0] {
                Value::Tag(t) => tag(t == FALSE),
                _ => return Err(Fault::Internal("`not` on a value that is not a tag")),
            },
            PrimOp::ShowInt => {
                let text = int(0)?.to_string();
                Value::Obj(self.heap.alloc(Payload::Str(text)))
            }
            PrimOp::StrConcat => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                Value::Obj(self.heap.alloc(Payload::Str(left + &right)))
            }
            // プリミティブは引数の所有権を受け取るので、比べた後に両方の文字列を手放す (`take_string`)
            PrimOp::StrEq | PrimOp::StrNe => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                tag((left == right) == (op == PrimOp::StrEq))
            }
            PrimOp::BoolEq | PrimOp::BoolNe => {
                let (Value::Tag(left), Value::Tag(right)) = (args[0], args[1]) else {
                    return Err(Fault::Internal(
                        "a `Bool` comparison on a value that is not a tag",
                    ));
                };
                tag((left == right) == (op == PrimOp::BoolEq))
            }
        })
    }

    /// `IO` はユーザーが handle できず、最下部の handler が必ずすぐに再開するので、継続を遡らずにその場で実行する
    /// (docs/spec/core-ir.md)。
    fn io(&mut self, op: IoOp, args: &[Value]) -> Result<Value, Fault> {
        match op {
            IoOp::Println => {
                let text = self.take_string(args[0])?;
                self.out
                    .write_str(&format!("{text}\n"))
                    .map_err(|error| Fault::Output(error.to_string()))?;
                Ok(Value::Unit)
            }
            IoOp::Open => {
                let path = self.take_string(args[0])?;
                // 絶対パスなら `join` がそのパスを返す
                let file = std::fs::File::open(self.file_root.join(&path)).map_err(|error| {
                    Fault::FileOpen {
                        path: path.clone(),
                        reason: io_reason(error.kind()),
                    }
                })?;
                let handle = FileHandle::new(path, Box::new(file));
                Ok(Value::Obj(self.heap.alloc(Payload::File(handle))))
            }
            // 受け取った `File` の参照を、そのまま返す組に移す (docs/spec/effects.md の「組み込みの `IO`」)
            IoOp::ReadAll => {
                let Value::Obj(file) = args[0] else {
                    return Err(Fault::Internal("`read_all` on a value that is not a file"));
                };
                let read = match self.heap.get_mut(file).map_err(Fault::Heap)? {
                    Payload::File(handle) => {
                        let mut bytes = Vec::new();
                        match handle.reader.read_to_end(&mut bytes) {
                            Ok(_) => String::from_utf8(bytes).map_err(|_| Fault::FileNotUtf8 {
                                path: handle.path.clone(),
                            }),
                            Err(error) => Err(Fault::FileRead {
                                path: handle.path.clone(),
                                reason: io_reason(error.kind()),
                            }),
                        }
                    }
                    _ => Err(Fault::Internal("`read_all` on a value that is not a file")),
                };
                let text = self.heap.alloc(Payload::Str(read?));
                Ok(Value::Obj(self.heap.alloc(Payload::Data {
                    tag: TUPLE,
                    fields: vec![Value::Obj(file), Value::Obj(text)],
                })))
            }
            // 破棄処理はオブジェクトの解放で、読み出し口を捨てると閉じる (docs/spec/runtime.md)
            IoOp::Close => {
                if let Value::Obj(file) = args[0] {
                    self.heap.decref(file).map_err(Fault::Heap)?;
                }
                Ok(Value::Unit)
            }
        }
    }

    /// プリミティブは引数の所有権を受け取るので、読んだ文字列は decref する。
    fn take_string(&mut self, value: Value) -> Result<String, Fault> {
        let Value::Obj(obj) = value else {
            return Err(Fault::Internal(
                "a string operation on a value that is not a string",
            ));
        };
        let text = match self.heap.get(obj).map_err(Fault::Heap)? {
            Payload::Str(text) => text.clone(),
            _ => {
                return Err(Fault::Internal(
                    "a string operation on a value that is not a string",
                ));
            }
        };
        self.heap.decref(obj).map_err(Fault::Heap)?;
        Ok(text)
    }

    fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        Err(RuntimeError::Leak(live))
    }
}

fn io_reason(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not found",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => "I/O error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_errors_name_the_fault_and_the_function() {
        let fault = |fault| {
            RuntimeError::Fault {
                fault,
                function: "f".to_string(),
            }
            .to_string()
        };
        assert_eq!(fault(Fault::DivisionByZero), "division by zero in `f`");
        assert_eq!(fault(Fault::IntegerOverflow), "integer overflow in `f`");
        assert_eq!(
            fault(Fault::Heap(HeapError::UseAfterFree)),
            "use of a freed object in `f`"
        );
        assert_eq!(
            fault(Fault::Output("broken pipe".to_string())),
            "cannot write the output: broken pipe in `f`"
        );
        assert_eq!(
            fault(Fault::Internal("a switch without a matching arm")),
            "internal error: a switch without a matching arm in `f`"
        );
    }

    #[test]
    fn leaks_are_displayed_with_the_object_counts() {
        let leak = RuntimeError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
        assert_eq!(
            leak.to_string(),
            "memory leak: objects were not freed: 2 Closure, 1 String"
        );
    }

    #[test]
    fn with_debug_heap_sets_the_flag() {
        assert!(RunConfig::default().with_debug_heap(true).debug_heap);
        assert!(!RunConfig::default().with_debug_heap(false).debug_heap);
    }
}
