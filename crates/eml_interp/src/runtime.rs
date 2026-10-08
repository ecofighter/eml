use std::cmp::Ordering;
use std::path::Path;

use eml_core_ir::{Atom, Call, CasePattern, FnIdx, VarId};
use eml_runtime::{Attachment, Closure, Frame, Heap, ObjRef, OutputSink, Payload, Value};

use crate::RunStats;
use crate::error::{Fault, RuntimeError};

/// 1つの命令を実行した後の状態。
pub(crate) enum Step {
    Continue,
    Finished,
}

/// 呼び出しと戻りの行き先。関数の入り方と、戻りのフレームの再開の番地の読み方は IR の形で決まるので、機械に任せる
/// (docs/spec/core-ir.md の「インタプリタ (CEK 機械)」)。
pub(crate) enum Transfer {
    /// 関数に入る。引数は関数の引数の順に並ぶ。
    Enter(FnIdx, Vec<Value>),
    /// 戻りのフレームを外し、呼び出し元に `value` を返す。`resume` と `saved` の鍵の意味は機械が決める。
    Resume {
        function: FnIdx,
        resume: u64,
        saved: Vec<(u32, Value)>,
        value: Value,
    },
    /// 値が継続の最下部の `Frame::Root` に届いた。
    Finished,
}

/// 関数値の適用の結果。関数に入るか、値ができたか (足りない引数のクロージャ)。
enum Applied {
    Enter(FnIdx, Vec<Value>),
    Value(Value),
}

/// 実行中の関数の環境。スロットは変数の番号で引き、読み出しはスロットを書き換えない。参照の所有は Core IR の命令
/// (消費、`dup`、`decref`、`release`) が表し、verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
pub(crate) struct Env(Vec<Option<Value>>);

impl Env {
    pub(crate) fn new(len: usize) -> Env {
        Env(vec![None; len])
    }

    /// 戻りのフレームに退避した値から、呼び出し元の環境を作り直す。
    pub(crate) fn restore(len: usize, saved: Vec<(u32, Value)>) -> Env {
        let mut env = Env::new(len);
        for (var, value) in saved {
            env.0[var as usize] = Some(value);
        }
        env
    }

    pub(crate) fn write(&mut self, var: VarId, value: Value) {
        self.0[var.0 as usize] = Some(value);
    }

    /// ヒープの値の所有権を渡すかどうかは Core IR の命令が決める (docs/spec/core-ir.md)。
    pub(crate) fn read(&self, var: VarId) -> Result<Value, Fault> {
        self.0[var.0 as usize].ok_or(Fault::Internal("a variable read before it was bound"))
    }

    pub(crate) fn atom(&self, atom: &Atom) -> Result<Value, Fault> {
        Ok(match *atom {
            Atom::Var(var) => self.read(var)?,
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
            Atom::Fn(function) => Value::Fn(function.0),
        })
    }

    pub(crate) fn atoms(&self, atoms: &[Atom]) -> Result<Vec<Value>, Fault> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    /// 呼び出しの後で使う変数だけをフレームに退避する。フレームは、ちょうど所有している参照だけを持つ
    /// (docs/spec/core-ir.md)。
    pub(crate) fn save(&self, vars: &[VarId]) -> Result<Vec<(u32, Value)>, Fault> {
        vars.iter()
            .map(|&var| Ok((var.0, self.read(var)?)))
            .collect()
    }
}

/// 機械の状態のうち、IR の形によらない部分。ヒープ、継続、handler の連鎖、入出力を持ち、呼び出し、戻り、エフェクト、
/// extern を実行する。制御と環境は機械が持つ。
pub(crate) struct Runtime<'p> {
    pub(crate) out: &'p OutputSink,
    pub(crate) file_root: &'p Path,
    pub(crate) heap: Heap,
    /// 継続の先頭のフレーム。最下部には常に `Frame::Root` がある。
    pub(crate) cont: ObjRef,
    /// handler の連鎖の先頭。`cont` から `next` でたどって最初に会う handler か `Mask` のフレームで、どちらもなければ
    /// `Root` のフレームである。`perform` はここから `outer` だけをたどる (docs/implementation/architecture.md の
    /// 「継続のフレーム」)。
    pub(crate) handlers: ObjRef,
    /// `Program::strings` の項目ごとの不死の物体。`Rhs::ConstString` は写さずに参照を1つ作る (docs/spec/runtime.md)。
    literals: Vec<ObjRef>,
    strings: &'p [String],
    /// 関数ごとの引数の数。関数値の適用が、足りない・ちょうど・余るを決める。
    arities: Vec<usize>,
    /// `find_handler` が調べたフレームの数 (`RunStats::handler_visits`)。
    pub(crate) handler_visits: u64,
}

impl<'p> Runtime<'p> {
    pub(crate) fn new(
        out: &'p OutputSink,
        file_root: &'p Path,
        strings: &'p [String],
        arities: Vec<usize>,
    ) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(Payload::Frame(Frame::Root));
        let literals = strings
            .iter()
            .map(|text| heap.alloc_immortal(Payload::Str(text.clone())))
            .collect();
        Runtime {
            out,
            file_root,
            heap,
            cont,
            handlers: cont,
            literals,
            strings,
            arities,
            handler_visits: 0,
        }
    }

    pub(crate) fn const_string(&mut self, index: u32) -> Result<Value, Fault> {
        let literal = self.literals[index as usize];
        self.heap.acquire_immortal(literal).map_err(Fault::Heap)?;
        Ok(Value::Obj(literal))
    }

    pub(crate) fn alloc(&mut self, payload: Payload) -> Value {
        Value::Obj(self.heap.alloc(payload))
    }

    /// 即値 (タグ、整数、関数の値) は RC を持たないので何もしない。
    pub(crate) fn dup(&mut self, value: Value) -> Result<(), Fault> {
        if let Value::Obj(obj) = value {
            self.heap.dup(obj).map_err(Fault::Heap)?;
        }
        Ok(())
    }

    /// 所有している参照を1つ手放す。継続も RC が1のオブジェクトなので、`drop k` もこれで解放される
    /// (docs/spec/core-ir.md)。
    pub(crate) fn decref(&mut self, value: Value) -> Result<(), Fault> {
        if let Value::Obj(obj) = value {
            self.heap.decref(obj).map_err(Fault::Heap)?;
        }
        Ok(())
    }

    /// `Switch` の scrutinee に合う case と、その case に入れるフィールド。scrutinee もフィールドも読むだけで、参照の
    /// 数を変えない。フィールドは scrutinee から借り、所有にするのは行き先の `dup` か `release` である
    /// (docs/spec/core-ir.md)。
    pub(crate) fn select_case<'c, C>(
        &self,
        value: Value,
        cases: &'c [C],
        pattern: impl Fn(&C) -> CasePattern,
    ) -> Result<(Option<&'c C>, Vec<Value>), Fault> {
        let find = |wanted: CasePattern| cases.iter().find(|case| pattern(case) == wanted);
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
        match self.heap.get(obj).map_err(Fault::Heap)? {
            // `default` はフィールドを束縛しないので、合う case があるときだけフィールドを渡す
            Payload::Data { tag, fields } => Ok(match find(CasePattern::Tag(*tag)) {
                Some(case) => (Some(case), fields.clone()),
                None => (None, Vec::new()),
            }),
            Payload::Str(text) => {
                let found = cases.iter().find(|case| {
                    matches!(pattern(case), CasePattern::String(index)
                        if self.strings[index as usize] == *text)
                });
                Ok((found, Vec::new()))
            }
            _ => Err(Fault::Internal(
                "a switch on an object that is neither data nor a string",
            )),
        }
    }

    /// 呼び出しのフレームを積む。末尾呼び出しは積まない。
    pub(crate) fn push_return(&mut self, function: FnIdx, resume: u64, saved: Vec<(u32, Value)>) {
        let frame = Frame::Return {
            function: function.0,
            resume,
            saved,
            next: self.cont,
        };
        self.cont = self.heap.alloc(Payload::Frame(frame));
    }

    /// 呼び出す。末尾でない呼び出しでは、機械が先に戻りのフレームを積んでいる。引数はフレームを積んだ後に読む。
    /// 環境の読み出しはスロットを書き換えないので、順序は結果に影響しない。
    pub(crate) fn call(&mut self, call: &Call, mask: &[u32], env: &Env) -> Result<Transfer, Fault> {
        // 戻りのフレームの上に積むので、呼び出し先が値を返すと先に外れる。末尾呼び出しでは戻りのフレームの代わりになる。
        // `resume` も、今の継続を読む前に積む (docs/implementation/architecture.md の「継続のフレーム」)
        if !mask.is_empty() {
            let frame = Frame::Mask {
                effects: mask.to_vec(),
                next: self.cont,
                outer: self.handlers,
            };
            self.cont = self.heap.alloc(Payload::Frame(frame));
            self.handlers = self.cont;
        }
        match call {
            Call::Direct(callee, args) => Ok(Transfer::Enter(*callee, env.atoms(args)?)),
            Call::Apply(callee, args) => {
                let callee = env.atom(callee)?;
                let args = env.atoms(args)?;
                self.apply_and_continue(callee, args)
            }
            Call::Handle {
                effect,
                init,
                body,
                clauses,
                ret: on_return,
            } => {
                let init = env.atom(init)?;
                let body = env.atom(body)?;
                let clauses = env.atoms(clauses)?;
                let on_return = env.atom(on_return)?;
                let frame = Frame::Handler {
                    effect: *effect,
                    clauses,
                    ret: on_return,
                    link: Attachment::Attached {
                        next: self.cont,
                        state: init,
                        outer: self.handlers,
                    },
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.handlers = self.cont;
                self.apply_and_continue(body, vec![Value::Unit])
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args,
            } => {
                let args = env.atoms(args)?;
                self.perform(*effect, *op, *resumable, args)
            }
            Call::Resume { k, arg, state } => {
                let k = env.atom(k)?;
                let arg = env.atom(arg)?;
                let state = env.atom(state)?;
                self.resume(k, arg, state)
            }
        }
    }

    /// 関数値を適用する。関数に入らずに値ができたら (足りない引数のクロージャ)、その値を継続に返す。
    pub(crate) fn apply_and_continue(
        &mut self,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Transfer, Fault> {
        match self.apply(callee, args)? {
            Applied::Enter(function, args) => Ok(Transfer::Enter(function, args)),
            Applied::Value(value) => self.ret(value),
        }
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
        let arity = self.arities[function as usize];
        let function = FnIdx(function);
        match all.len().cmp(&arity) {
            Ordering::Equal => Ok(Applied::Enter(function, all)),
            Ordering::Less => {
                let closure = Closure {
                    function: function.0,
                    args: all,
                };
                Ok(Applied::Value(self.alloc(Payload::Closure(closure))))
            }
            Ordering::Greater => {
                let rest = all.split_off(arity);
                let frame = Frame::Apply {
                    args: rest,
                    next: self.cont,
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                Ok(Applied::Enter(function, all))
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

    /// 継続の先頭のフレームに値を返す。余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    pub(crate) fn ret(&mut self, mut value: Value) -> Result<Transfer, Fault> {
        loop {
            let top = self.cont;
            // フレームはつねに一意である。共有されうるのは継続オブジェクトだけで、再開するときに区間を写す (docs/spec/runtime.md)
            let Payload::Frame(frame) = self.heap.take(top).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            match frame {
                Frame::Apply { args, next } => {
                    self.cont = next;
                    match self.apply(value, args)? {
                        Applied::Enter(function, args) => {
                            return Ok(Transfer::Enter(function, args));
                        }
                        Applied::Value(result) => value = result,
                    }
                }
                Frame::Return {
                    function,
                    resume,
                    saved,
                    next,
                } => {
                    self.cont = next;
                    return Ok(Transfer::Resume {
                        function: FnIdx(function),
                        resume,
                        saved,
                        value,
                    });
                }
                // 本体が値を返したので handler を外す。節のクロージャはもう呼ばない
                Frame::Handler {
                    effect: _,
                    clauses,
                    ret: on_return,
                    link,
                } => {
                    for clause in clauses {
                        self.decref(clause)?;
                    }
                    let Attachment::Attached { next, state, outer } = link else {
                        return Err(Fault::Internal("a detached handler received a value"));
                    };
                    self.pop_chain(top, outer)?;
                    self.cont = next;
                    match self.apply(on_return, vec![value, state])? {
                        Applied::Enter(function, args) => {
                            return Ok(Transfer::Enter(function, args));
                        }
                        Applied::Value(result) => value = result,
                    }
                }
                // 値はそのまま外側へ返す
                Frame::Mask {
                    effects: _,
                    next,
                    outer,
                } => {
                    self.pop_chain(top, outer)?;
                    self.cont = next;
                }
                Frame::Root => {
                    self.decref(value)?;
                    return Ok(Transfer::Finished);
                }
            }
        }
    }

    /// 外す handler か `Mask` のフレームを連鎖から外す。外すフレームは連鎖の先頭のはずなので、O(1) で確かめる
    /// (docs/implementation/architecture.md の「継続のフレーム」)。
    fn pop_chain(&mut self, frame: ObjRef, outer: ObjRef) -> Result<(), Fault> {
        if frame != self.handlers {
            return Err(Fault::Internal(
                "a popped handler or mask is not the head of the handler chain",
            ));
        }
        self.handlers = outer;
        Ok(())
    }

    pub(crate) fn stats(&self) -> RunStats {
        RunStats {
            handler_visits: self.handler_visits,
            string_bytes_copied: self.heap.string_bytes_written(),
            rc_increments: self.heap.rc_increments(),
            rc_decrements: self.heap.rc_decrements(),
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
