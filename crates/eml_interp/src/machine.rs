use std::cmp::Ordering;
use std::path::Path;

use eml_core_ir::{Atom, CExpr, CExprId, Call, Case, CasePattern, FnIdx, Program, Rhs, VarId};
use eml_runtime::{Closure, Frame, Heap, Link, ObjRef, OutputSink, Payload, Value};

use crate::RunStats;
use crate::error::{Fault, RuntimeError};

/// 関数値の適用の結果。関数に入ったか、値ができたか (足りない引数のクロージャ)。
enum Applied {
    Entered,
    Value(Value),
}

/// 1つの命令を実行した後の状態。
pub(crate) enum Step {
    Continue,
    Finished,
}

/// 呼び出しから戻った後に再開するところと、呼び出しのフレームに退避する変数。
struct ReturnPoint<'p> {
    bind: VarId,
    control: CExprId,
    saved: &'p [VarId],
}

/// CEK 機械。制御 (`function` と `control`)、環境 (`slots`)、継続 (`cont`) からなる。
pub(crate) struct Machine<'p> {
    pub(crate) program: &'p Program,
    pub(crate) out: &'p OutputSink,
    pub(crate) file_root: &'p Path,
    pub(crate) heap: Heap,
    function: FnIdx,
    control: CExprId,
    /// 今の関数の環境。読み出しはスロットを書き換えない。参照の所有は Core IR の命令 (使用、`dup`、`decref`) が表し、
    /// verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
    slots: Vec<Option<Value>>,
    /// 継続の先頭のフレーム。最下部には常に `Frame::Root` がある。
    pub(crate) cont: ObjRef,
    /// `Program::strings` の項目ごとの不死の物体。`Rhs::ConstString` は写さずに参照を1つ作る (docs/spec/runtime.md)。
    literals: Vec<ObjRef>,
    /// `find_handler` が調べたフレームの数 (`RunStats::handler_visits`)。
    pub(crate) handler_visits: u64,
}

impl<'p> Machine<'p> {
    pub(crate) fn new(program: &'p Program, out: &'p OutputSink, file_root: &'p Path) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(Payload::Frame(Frame::Root));
        let literals = program
            .strings
            .iter()
            .map(|text| heap.alloc_immortal(Payload::Str(text.clone())))
            .collect();
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
            literals,
            handler_visits: 0,
        }
    }

    pub(crate) fn run(&mut self) -> Result<(), RuntimeError> {
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
            CExpr::Switch {
                scrutinee,
                cases,
                default,
            } => {
                let value = self.atom(scrutinee)?;
                let (case, fields) = self.select_case(value, cases)?;
                match case {
                    Some(case) => {
                        if case.fields.len() != fields.len() {
                            return Err(Fault::Internal(
                                "a switch case binds a different number of fields than the value has",
                            ));
                        }
                        for (&field, value) in case.fields.iter().zip(fields) {
                            self.slots[field.0 as usize] = Some(value);
                        }
                        self.control = case.body;
                    }
                    None => {
                        self.control =
                            default.ok_or(Fault::Internal("a switch without a matching case"))?;
                    }
                }
            }
            CExpr::Return(atom) => {
                let value = self.atom(atom)?;
                return self.ret(value);
            }
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            CExpr::TailCall { call, mask } => return self.call(call, mask, None),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body: _,
                scope,
            } => self.control = *scope,
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
                let literal = self.literals[*index as usize];
                self.heap.acquire_immortal(literal).map_err(Fault::Heap)?;
                Value::Obj(literal)
            }
            Rhs::Extern(e, args) => {
                let args = self.atoms(args)?;
                self.call_extern(*e, &args)?
            }
            // 所有している参照を1つ手放す。継続も RC が1のオブジェクトなので、これで解放される (docs/spec/core-ir.md)
            Rhs::Drop(atom) => {
                if let Value::Obj(obj) = self.atom(atom)? {
                    self.heap.decref(obj).map_err(Fault::Heap)?;
                }
                Value::Unit
            }
            Rhs::Call { call, mask, saved } => {
                let ret = ReturnPoint {
                    bind: var,
                    control: body,
                    saved,
                };
                return self.call(call, mask, Some(ret));
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

    /// `Switch` の scrutinee に合う case と、その case に入れるフィールド。scrutinee は move で受け取る。合う case の
    /// ある `data` の値は、`take_or_copy` でフィールドを複製して箱を手放すので、共有されていても case はフィールドの
    /// 参照を1つずつ所有して始まる。比べ終えた文字列と、`default` に進む `data` の値は、分解せずに手放す。`default`
    /// はフィールドを束縛しないので、手放すフィールドが残らない (docs/spec/core-ir.md)。
    fn select_case(
        &mut self,
        value: Value,
        cases: &'p [Case],
    ) -> Result<(Option<&'p Case>, Vec<Value>), Fault> {
        let program = self.program;
        let find = |pattern: CasePattern| cases.iter().find(|case| case.pattern == pattern);
        let obj = match value {
            Value::Tag(tag) => return Ok((find(CasePattern::Tag(tag)), Vec::new())),
            Value::Int(n) => return Ok((find(CasePattern::Int(n)), Vec::new())),
            Value::Obj(obj) => obj,
            _ => {
                return Err(Fault::Internal(
                    "a switch on a value that is not a tag, an integer or a string",
                ));
            }
        };
        let found = match self.heap.get(obj).map_err(Fault::Heap)? {
            Payload::Data { tag, .. } => find(CasePattern::Tag(*tag)),
            Payload::Str(text) => cases.iter().find(|case| {
                matches!(case.pattern, CasePattern::String(index)
                    if program.strings[index as usize] == *text)
            }),
            _ => {
                return Err(Fault::Internal(
                    "a switch on an object that is neither data nor a string",
                ));
            }
        };
        if let Some(case) = found
            && matches!(case.pattern, CasePattern::Tag(_))
        {
            return match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
                Payload::Data { fields, .. } => Ok((Some(case), fields)),
                _ => Err(Fault::Internal("a switch on an object that is not data")),
            };
        }
        self.heap.decref(obj).map_err(Fault::Heap)?;
        Ok((found, Vec::new()))
    }

    /// 呼び出す。`ret` は戻った値を受ける変数と再開する位置で、`None` ならフレームを積まない (末尾呼び出し)。
    /// 引数はフレームを積んだ後に読む。`push_frame` は環境のスロットを読むだけで書き換えないので、順序は結果に影響しない。
    fn call(
        &mut self,
        call: &Call,
        mask: &[u32],
        ret: Option<ReturnPoint<'p>>,
    ) -> Result<Step, Fault> {
        if let Some(ret) = ret {
            self.push_frame(ret)?;
        }
        // 戻りのフレームの上に積むので、呼び出し先が値を返すと先に外れる。末尾呼び出しでは戻りのフレームの代わりになる。
        // `resume` も、今の継続を読む前に積む (docs/implementation/architecture.md の「継続のフレーム」)
        if !mask.is_empty() {
            let frame = Frame::Mask {
                effects: mask.to_vec(),
                next: self.cont,
            };
            self.cont = self.heap.alloc(Payload::Frame(frame));
        }
        match call {
            Call::Direct(callee, args) => {
                let args = self.atoms(args)?;
                self.enter(*callee, args);
                Ok(Step::Continue)
            }
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                let args = self.atoms(args)?;
                self.apply_and_continue(callee, args)
            }
            Call::Handle {
                effect,
                init,
                body,
                clauses,
                ret: on_return,
            } => {
                let init = self.atom(init)?;
                let body = self.atom(body)?;
                let clauses = self.atoms(clauses)?;
                let on_return = self.atom(on_return)?;
                let frame = Frame::Handler {
                    effect: *effect,
                    clauses,
                    ret: on_return,
                    link: Some(Link {
                        next: self.cont,
                        state: init,
                    }),
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.apply_and_continue(body, vec![Value::Unit])
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args,
            } => {
                let args = self.atoms(args)?;
                self.perform(*effect, *op, *resumable, args)
            }
            Call::Resume { k, arg, state } => {
                let k = self.atom(k)?;
                let arg = self.atom(arg)?;
                let state = self.atom(state)?;
                self.resume(k, arg, state)
            }
        }
    }

    /// 関数値を適用する。関数に入らずに値ができたら (足りない引数のクロージャ)、その値を継続に返す。
    pub(crate) fn apply_and_continue(
        &mut self,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Step, Fault> {
        match self.apply(callee, args)? {
            Applied::Entered => Ok(Step::Continue),
            Applied::Value(value) => self.ret(value),
        }
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
        let (function, mut all) = match callee {
            Value::Fn(function) => (function, Vec::new()),
            Value::Obj(obj) => {
                let closure = self.take_closure(obj)?;
                (closure.function, closure.args)
            }
            _ => return Err(Fault::Internal("applying a value that is not a function")),
        };
        all.append(&mut args);
        let function = FnIdx(function);
        let arity = self.program.function(function).params.len();
        match all.len().cmp(&arity) {
            Ordering::Equal => {
                self.enter(function, all);
                Ok(Applied::Entered)
            }
            Ordering::Less => {
                let closure = Closure {
                    function: function.0,
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
    fn push_frame(&mut self, ret: ReturnPoint<'p>) -> Result<(), Fault> {
        let saved = ret
            .saved
            .iter()
            .map(|&var| Ok((var.0, self.read(var)?)))
            .collect::<Result<Vec<_>, Fault>>()?;
        let frame = Frame::Return {
            function: self.function.0,
            resume: ret.control.0,
            bind: ret.bind.0,
            saved,
            next: self.cont,
        };
        self.cont = self.heap.alloc(Payload::Frame(frame));
        Ok(())
    }

    /// 継続の先頭のフレームに値を返す。最下部の `Frame::Root` に届いたら、プログラムが終わる。
    /// 余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    pub(crate) fn ret(&mut self, mut value: Value) -> Result<Step, Fault> {
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
                    resume: control,
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
                    self.control = CExprId(control);
                    self.cont = next;
                    return Ok(Step::Continue);
                }
                // 本体が値を返したので handler を外す。節のクロージャはもう呼ばない
                Frame::Handler {
                    effect: _,
                    clauses,
                    ret: on_return,
                    link,
                } => {
                    for clause in clauses {
                        if let Value::Obj(obj) = clause {
                            self.heap.decref(obj).map_err(Fault::Heap)?;
                        }
                    }
                    let Link { next, state } =
                        link.ok_or(Fault::Internal("a detached handler received a value"))?;
                    self.cont = next;
                    match self.apply(on_return, vec![value, state])? {
                        Applied::Entered => return Ok(Step::Continue),
                        Applied::Value(result) => value = result,
                    }
                }
                // 値はそのまま外側へ返す
                Frame::Mask { effects: _, next } => self.cont = next,
                Frame::Root => {
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
            Atom::Fn(function) => Value::Fn(function.0),
        })
    }

    fn atoms(&self, atoms: &[Atom]) -> Result<Vec<Value>, Fault> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    pub(crate) fn stats(&self) -> RunStats {
        RunStats {
            handler_visits: self.handler_visits,
            string_bytes_copied: self.heap.string_bytes_written(),
        }
    }

    pub(crate) fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        Err(RuntimeError::Leak(live))
    }
}
