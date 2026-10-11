use std::cmp::Ordering;
use std::path::Path;

use eml_lir::{Atom, Call, CasePattern, FnIdx, VarId};
use eml_runtime::{Closure, Frame, Heap, HeapError, ObjRef, OutputSink, Payload, Value};

use crate::RunStats;
use crate::error::{Fault, RuntimeError};

/// 1つの命令を実行した後の状態。
pub(crate) enum Step {
    Continue,
    Finished,
}

/// yield の状態 (docs/spec/lir.md の「実行の文脈」)。
pub(crate) enum YieldState {
    None,
    /// yield 中か、`abort` なら中断中である。`target` (宛先の evidence)、`clause` (節の本体)、`cont`
    /// (組み立て中の継続 κ) は参照を1つずつ所有する。`cont` は取り出している間 `Value::Unit` である。
    Yielding {
        target: ObjRef,
        clause: Value,
        cont: Value,
        abort: bool,
    },
}

/// 呼び出しと戻りの行き先。関数の入り方と、戻りのフレームの再開の番地の読み方は IR の形で決まるので、機械に任せる
/// (docs/spec/lir.md の「インタプリタ (CEK 機械)」)。
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

/// 実行中の関数の環境。スロットは変数の番号で引き、読み出しはスロットを書き換えない。参照の所有は LIR の命令
/// (消費、`dup`、`decref`、`release`) が表し、verifier がその釣り合いを確かめる (docs/spec/lir.md)。
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

    /// ヒープの値の所有権を渡すかどうかは LIR の命令が決める (docs/spec/lir.md)。
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
    /// (docs/spec/lir.md)。
    pub(crate) fn save(&self, vars: &[VarId]) -> Result<Vec<(u32, Value)>, Fault> {
        vars.iter()
            .map(|&var| Ok((var.0, self.read(var)?)))
            .collect()
    }
}

/// 機械の状態のうち、IR の形によらない部分。ヒープ、継続、evidence のリスト、yield の状態、入出力を持ち、呼び出し、
/// 戻り、基本操作、extern を実行する。制御と環境は機械が持つ。
pub(crate) struct Runtime<'p> {
    pub(crate) out: &'p OutputSink,
    pub(crate) file_root: &'p Path,
    pub(crate) heap: Heap,
    /// 継続の先頭のフレーム。最下部には常に `Frame::Root` がある。
    pub(crate) cont: ObjRef,
    /// `Program::strings` の項目ごとの不死の物体。`Rhs::ConstString` は写さずに参照を1つ作る (docs/spec/runtime.md)。
    literals: Vec<ObjRef>,
    strings: &'p [String],
    /// 関数ごとの引数の数。関数値の適用が、足りない・ちょうど・余るを決める。
    arities: Vec<usize>,
    /// エフェクトごとの操作の数。`ev_new` が入口の表の欄をこの数だけ作る。
    pub(crate) effect_sizes: Vec<usize>,
    /// 今有効な evidence のリスト。空のリストは `Value::Unit` である。関数の引数でなく実行の文脈に持つので、yield
    /// しうる関数とそうでない関数の呼び出しの規約は同じになる
    /// (docs/spec/lir.md の「実行の文脈」)。
    pub(crate) evv: Value,
    pub(crate) yield_state: YieldState,
    /// `ev_lookup` と `evv_mask` がたどった `evv` の節点の数。
    pub(crate) evidence_steps: u64,
    /// `yield_to` と `yield_abort` を実行した回数。
    pub(crate) yields: u64,
    /// `yield_cont_set` を実行した回数。継続 κ に足したフレームの数である。
    pub(crate) yield_frames: u64,
    /// 機械が実行した `box` の数 (`RunStats::boxes`)。
    pub(crate) boxes: u64,
    /// 機械が実行した `unbox` の数 (`RunStats::unboxes`)。
    pub(crate) unboxes: u64,
}

impl<'p> Runtime<'p> {
    pub(crate) fn new(
        out: &'p OutputSink,
        file_root: &'p Path,
        strings: &'p [String],
        arities: Vec<usize>,
        effect_sizes: Vec<usize>,
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
            literals,
            strings,
            arities,
            effect_sizes,
            evv: Value::Unit,
            yield_state: YieldState::None,
            evidence_steps: 0,
            yields: 0,
            yield_frames: 0,
            boxes: 0,
            unboxes: 0,
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
    /// (docs/spec/lir.md)。
    pub(crate) fn decref(&mut self, value: Value) -> Result<(), Fault> {
        if let Value::Obj(obj) = value {
            self.heap.decref(obj).map_err(Fault::Heap)?;
        }
        Ok(())
    }

    /// `Switch` の scrutinee に合う case と、その case に入れるフィールド。scrutinee もフィールドも読むだけで、参照の
    /// 数を変えない。フィールドは scrutinee から借りる。所有にする `dup` や `release` は、Perceus が所有の要る所に
    /// 置く (docs/spec/lir.md の「所有を取る時点」)。
    pub(crate) fn select_case<'s, 'c, C>(
        &'s self,
        value: Value,
        cases: &'c [C],
        pattern: impl Fn(&C) -> CasePattern,
    ) -> Result<(Option<&'c C>, &'s [Value]), Fault> {
        let find = |wanted: CasePattern| cases.iter().find(|case| pattern(case) == wanted);
        let obj = match value {
            Value::Tag(tag) => return Ok((find(CasePattern::Tag(tag)), &[])),
            Value::Int(n) => return Ok((find(CasePattern::Int(n)), &[])),
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
                Some(case) => (Some(case), fields),
                None => (None, &[]),
            }),
            Payload::Str(text) => {
                let found = cases.iter().find(|case| {
                    matches!(pattern(case), CasePattern::String(index)
                        if self.strings[index as usize] == *text)
                });
                Ok((found, &[]))
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
    /// 環境の読み出しはスロットを書き換えないので、順序は結果に影響しない。エフェクトの命令は EP が基本操作に置き換える
    /// ので、ここに届けば内部の誤りである (docs/spec/lir.md の「インタプリタ (CEK 機械)」)。
    pub(crate) fn call(&mut self, call: &Call, env: &Env) -> Result<Transfer, Fault> {
        match call {
            Call::Direct(callee, args) => Ok(Transfer::Enter(*callee, env.atoms(args)?)),
            Call::Apply {
                callee,
                args,
                yields: _,
            } => {
                let callee = env.atom(callee)?;
                let args = env.atoms(args)?;
                self.apply_and_continue(callee, args)
            }
            // 節の呼び出しは、インタプリタでは `apply` と同じである
            // (docs/spec/lir.md の「節の呼び出し」)
            Call::Clause {
                effect: _,
                op: _,
                callee,
                args,
            } => {
                let callee = env.atom(callee)?;
                let args = env.atoms(args)?;
                self.apply_and_continue(callee, args)
            }
        }
    }

    /// 関数値を適用する。関数に入らずに値ができたら (足りない引数のクロージャ)、その値を継続に返す。
    fn apply_and_continue(&mut self, callee: Value, args: Vec<Value>) -> Result<Transfer, Fault> {
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
        match self.heap.take_or_copy(obj) {
            Ok(Payload::Closure(closure)) => Ok(closure),
            Ok(_) | Err(HeapError::NotCopyable) => {
                Err(Fault::Internal("applying an object that is not a closure"))
            }
            Err(error) => Err(Fault::Heap(error)),
        }
    }

    /// 継続の先頭のフレームに値を返す。余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    pub(crate) fn ret(&mut self, mut value: Value) -> Result<Transfer, Fault> {
        loop {
            let top = self.cont;
            // フレームはつねに一意である。フレームを捕まえる命令はなく、再開する継続はクロージャで作る
            // (docs/spec/core-ir.md の「継続の組み立て」)
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
                Frame::Root => {
                    self.decref(value)?;
                    return Ok(Transfer::Finished);
                }
            }
        }
    }

    pub(crate) fn stats(&self) -> RunStats {
        RunStats {
            evidence_steps: self.evidence_steps,
            yields: self.yields,
            yield_frames: self.yield_frames,
            string_bytes_copied: self.heap.string_bytes_written(),
            rc_increments: self.heap.rc_increments(),
            rc_decrements: self.heap.rc_decrements(),
            boxes: self.boxes,
            unboxes: self.unboxes,
            peak_objects: self.heap.peak_objects(),
        }
    }

    /// 正しく終わった実行は、evidence のリストを空に戻し、yield を終えている。残っていると、その中の物体が
    /// リークに見えるので、リークの検査より先に確かめる。
    pub(crate) fn check_context(&self) -> Result<(), Fault> {
        if self.evv != Value::Unit {
            return Err(Fault::Internal("the evidence list is not empty at the end"));
        }
        if !matches!(self.yield_state, YieldState::None) {
            return Err(Fault::Internal("a yield is left at the end"));
        }
        Ok(())
    }

    pub(crate) fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        Err(RuntimeError::Leak(live))
    }
}
