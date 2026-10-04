use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId,
};
use eml_runtime::{
    ApplyFrame, Closure, DescId, Frame, Heap, HeapError, ObjRef, OutputSink, Owned, Payload, Value,
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
}

/// 将来 `threads` などを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> Self {
        self.debug_heap = debug_heap;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError(pub String);

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

pub fn run(
    program: Arc<Program>,
    config: &RunConfig,
    out: &OutputSink,
) -> Result<(), RuntimeError> {
    let mut machine = Machine::new(&program, out);
    machine.run()?;
    // 実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから
    if config.debug_heap {
        machine.check_leaks()?;
    }
    Ok(())
}

/// 継続の最下部にある、`IO` の組み込み handler のフレームの印 (docs/spec/core-ir.md)。
const IO_HANDLER: u32 = u32::MAX;

/// CEK 機械。制御 (`function` と `control`)、環境 (`slots`)、継続 (`cont`) からなる。
struct Machine<'p> {
    program: &'p Program,
    out: &'p OutputSink,
    heap: Heap,
    function: FnIdx,
    control: CExprId,
    slots: Vec<Option<Owned>>,
    /// 継続の先頭のフレーム。最下部には常に `IO` の handler のフレームがある。
    cont: ObjRef,
}

impl<'p> Machine<'p> {
    fn new(program: &'p Program, out: &'p OutputSink) -> Self {
        let mut heap = Heap::new();
        let mut cont = heap.alloc(
            DescId::FRAME,
            Payload::Frame(Frame {
                function: IO_HANDLER,
                resume: 0,
                bind: 0,
                slots: None,
                next: None,
            }),
        );
        let main = program.function(program.main);
        // `main` の型は `Unit -> <IO> Unit` に決まっている (docs/spec/types.md)。等式に引数がない `main = fn () -> ...` は
        // 関数値を返すので、返った値に `()` を適用するフレームを先に積んでおく
        if main.params.is_empty() {
            cont = heap.alloc(
                DescId::FRAME,
                Payload::ApplyFrame(ApplyFrame {
                    args: vec![Value::Unit],
                    next: Some(cont),
                }),
            );
        }
        let mut slots = vec![None; main.vars.len()];
        // `main : Unit -> <IO> Unit` の引数
        for param in &main.params {
            slots[param.0 as usize] = Some(Owned::new(Value::Unit));
        }
        Machine {
            program,
            out,
            heap,
            function: program.main,
            control: main.body,
            slots,
            cont,
        }
    }

    fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(message) => {
                    let function = &self.program.function(self.function).name;
                    return Err(RuntimeError(format!("{message} in `{function}`")));
                }
            }
        }
    }

    /// 1つの命令を実行する。プログラムが終わったら真を返す。
    fn step(&mut self) -> Result<bool, String> {
        let program = self.program;
        match program.function(self.function).expr(self.control) {
            CExpr::Let { var, rhs, body } => return self.bind(*var, rhs, *body),
            CExpr::Switch { scrutinee, arms } => {
                let Value::Tag(tag) = self.atom(scrutinee)? else {
                    return Err(internal("a switch on a value that is not a tag"));
                };
                self.control = arms
                    .iter()
                    .find(|(arm_tag, _)| *arm_tag == tag)
                    .map(|&(_, arm)| arm)
                    .ok_or_else(|| internal("a switch without a matching arm"))?;
            }
            CExpr::Return(atom) => {
                let value = self.atom(atom)?;
                return self.ret(value);
            }
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            CExpr::TailCall(call) => return self.call(call, None),
            CExpr::Join { scope, .. } => self.control = *scope,
            CExpr::Jump { join, arg } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let value = self.atom(arg)?;
                let (param, body) = program.function(self.function).join(*join);
                self.slots[param.0 as usize] = Some(Owned::new(value));
                self.control = body;
            }
            CExpr::Dup { var, body } => {
                let slot = self.slots[var.0 as usize]
                    .as_mut()
                    .ok_or_else(|| internal("a variable duplicated after it was moved"))?;
                if let Value::Obj(obj) = slot.value {
                    self.heap.dup(obj).map_err(heap_error)?;
                    slot.refs += 1;
                }
                self.control = *body;
            }
            CExpr::Decref { var, body } => {
                if let Value::Obj(obj) = self.atom(&Atom::Var(*var))? {
                    self.heap.decref(obj).map_err(heap_error)?;
                }
                self.control = *body;
            }
        }
        Ok(false)
    }

    fn bind(&mut self, var: VarId, rhs: &Rhs, body: CExprId) -> Result<bool, String> {
        let value = match rhs {
            Rhs::Atom(atom) => self.atom(atom)?,
            Rhs::ConstString(index) => {
                let text = self.program.strings[*index as usize].clone();
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(text)))
            }
            Rhs::Prim(op, args) => {
                let args = self.atoms(args)?;
                self.prim(*op, &args)?
            }
            Rhs::Perform(IoOp::Println, args) => {
                // `IO` はユーザーが handle できず、最下部の handler が必ずすぐに再開するので、継続を遡らずにその場で
                // 実行する (docs/spec/core-ir.md)
                let args = self.atoms(args)?;
                let text = self.take_string(args[0])?;
                self.out
                    .write_str(&format!("{text}\n"))
                    .map_err(|error| format!("cannot write the output: {error}"))?;
                Value::Unit
            }
            Rhs::Call(call) => return self.call(call, Some((var, body))),
            Rhs::MakeClosure(function, args) => {
                let args = self.atoms(args)?;
                let closure = Closure {
                    function: function.0,
                    args,
                };
                Value::Obj(self.heap.alloc(DescId::CLOSURE, Payload::Closure(closure)))
            }
        };
        self.slots[var.0 as usize] = Some(Owned::new(value));
        self.control = body;
        Ok(false)
    }

    /// 呼び出す。引数は、環境を退避する前に読む。`resume` は、戻った値を受ける変数と再開する位置で、`None` なら
    /// フレームを積まない (末尾呼び出し)。
    fn call(&mut self, call: &Call, resume: Option<(VarId, CExprId)>) -> Result<bool, String> {
        let prepared = match call {
            Call::Direct(callee, args) => Prepared::Direct(*callee, self.atoms(args)?),
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                Prepared::Apply(callee, self.atoms(args)?)
            }
        };
        if let Some((var, body)) = resume {
            self.push_frame(var, body);
        }
        match prepared {
            Prepared::Direct(callee, args) => {
                self.enter(callee, args);
                Ok(false)
            }
            Prepared::Apply(callee, args) => match self.apply(callee, args)? {
                Applied::Entered => Ok(false),
                Applied::Value(value) => self.ret(value),
            },
        }
    }

    fn enter(&mut self, callee: FnIdx, args: Vec<Value>) {
        let target = self.program.function(callee);
        let mut slots = vec![None; target.vars.len()];
        for (param, value) in target.params.iter().zip(args) {
            slots[param.0 as usize] = Some(Owned::new(value));
        }
        self.slots = slots;
        self.function = callee;
        self.control = target.body;
    }

    /// 関数値を引数に適用する (docs/spec/core-ir.md の eval/apply)。引数の個数が揃えば関数に入り、足りなければ
    /// 引数を足したクロージャを値にし、余れば余りを持つフレームを積んでから関数に入る。
    fn apply(&mut self, callee: Value, mut args: Vec<Value>) -> Result<Applied, String> {
        let Value::Obj(obj) = callee else {
            return Err(internal("applying a value that is not a closure"));
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
                let value = self.heap.alloc(DescId::CLOSURE, Payload::Closure(closure));
                Ok(Applied::Value(Value::Obj(value)))
            }
            Ordering::Greater => {
                let rest = all.split_off(arity);
                let frame = ApplyFrame {
                    args: rest,
                    next: Some(self.cont),
                };
                self.cont = self.heap.alloc(DescId::FRAME, Payload::ApplyFrame(frame));
                self.enter(function, all);
                Ok(Applied::Entered)
            }
        }
    }

    /// 呼び出しはクロージャの所有権を受け取る。一意なら中身を取り出し、共有されていれば中身の参照を複製してから
    /// 手放す。
    fn take_closure(&mut self, obj: ObjRef) -> Result<Closure, String> {
        if self.heap.is_unique(obj).map_err(heap_error)? {
            return match self.heap.take(obj).map_err(heap_error)? {
                Payload::Closure(closure) => Ok(closure),
                _ => Err(internal("applying an object that is not a closure")),
            };
        }
        let closure = match self.heap.get(obj).map_err(heap_error)? {
            Payload::Closure(closure) => closure.clone(),
            _ => return Err(internal("applying an object that is not a closure")),
        };
        for value in &closure.args {
            if let Value::Obj(captured) = value {
                self.heap.dup(*captured).map_err(heap_error)?;
            }
        }
        self.heap.decref(obj).map_err(heap_error)?;
        Ok(closure)
    }

    /// 呼び出しでは環境ごと退避する。
    fn push_frame(&mut self, bind: VarId, resume: CExprId) {
        let frame = Frame {
            function: self.function.0,
            resume: resume.0,
            bind: bind.0,
            slots: Some(std::mem::take(&mut self.slots)),
            next: Some(self.cont),
        };
        self.cont = self.heap.alloc(DescId::FRAME, Payload::Frame(frame));
    }

    /// 継続の先頭のフレームに値を返す。最下部の `IO` の handler に届いたら、プログラムが終わる。
    /// 余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    fn ret(&mut self, mut value: Value) -> Result<bool, String> {
        loop {
            // 段階2までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う
            let frame = match self.heap.take(self.cont).map_err(heap_error)? {
                Payload::ApplyFrame(frame) => {
                    self.cont = frame
                        .next
                        .ok_or_else(|| internal("an apply frame without a next frame"))?;
                    match self.apply(value, frame.args)? {
                        Applied::Entered => return Ok(false),
                        Applied::Value(result) => {
                            value = result;
                            continue;
                        }
                    }
                }
                Payload::Frame(frame) => frame,
                _ => return Err(internal("the continuation is not a frame")),
            };
            if frame.function == IO_HANDLER {
                if let Value::Obj(obj) = value {
                    self.heap.decref(obj).map_err(heap_error)?;
                }
                return Ok(true);
            }
            if let Some(slots) = frame.slots {
                self.slots = slots;
            }
            self.function = FnIdx(frame.function);
            self.control = CExprId(frame.resume);
            self.slots[frame.bind as usize] = Some(Owned::new(value));
            self.cont = frame
                .next
                .ok_or_else(|| internal("a frame without a next frame"))?;
            return Ok(false);
        }
    }

    /// ヒープの値の読み出しは所有権の移動で、複製は `dup` 命令だけが行う (docs/spec/core-ir.md)。ただし Perceus の `dup` で
    /// 1つの変数が複数の参照を持つので (`s ++ s`、`dup s; return s`)、環境を共有する入れ子の式でも後で読めるように
    /// 変数ごとに参照の数を数え、読むたびに1つ減らして0になったらスロットを空にする。ヒープにない値は何度でも読める。
    fn atom(&mut self, atom: &Atom) -> Result<Value, String> {
        Ok(match *atom {
            Atom::Var(var) => {
                let slot = &mut self.slots[var.0 as usize];
                let owned = slot
                    .as_mut()
                    .ok_or_else(|| internal("a variable read after it was moved"))?;
                let value = owned.value;
                if let Value::Obj(_) = value {
                    owned.refs -= 1;
                    if owned.refs == 0 {
                        *slot = None;
                    }
                }
                value
            }
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
        })
    }

    fn atoms(&mut self, atoms: &[Atom]) -> Result<Vec<Value>, String> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    fn prim(&mut self, op: PrimOp, args: &[Value]) -> Result<Value, String> {
        let int = |index: usize| match args[index] {
            Value::Int(n) => Ok(n),
            _ => Err(internal(
                "an integer operation on a value that is not an integer",
            )),
        };
        let overflow = || "integer overflow".to_string();
        let tag = |b: bool| Value::Tag(if b { TRUE } else { FALSE });
        Ok(match op {
            PrimOp::IntAdd => Value::Int(int(0)?.checked_add(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntSub => Value::Int(int(0)?.checked_sub(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntMul => Value::Int(int(0)?.checked_mul(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntDiv | PrimOp::IntMod => {
                let (a, b) = (int(0)?, int(1)?);
                if b == 0 {
                    return Err("division by zero".to_string());
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
                _ => return Err(internal("`not` on a value that is not a tag")),
            },
            PrimOp::ShowInt => {
                let text = int(0)?.to_string();
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(text)))
            }
            PrimOp::StrConcat => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(left + &right)))
            }
        })
    }

    /// プリミティブは引数の所有権を受け取るので、読んだ文字列は decref する。
    fn take_string(&mut self, value: Value) -> Result<String, String> {
        let Value::Obj(obj) = value else {
            return Err(internal(
                "a string operation on a value that is not a string",
            ));
        };
        let text = match self.heap.get(obj).map_err(heap_error)? {
            Payload::Str(text) => text.clone(),
            _ => {
                return Err(internal(
                    "a string operation on a value that is not a string",
                ));
            }
        };
        self.heap.decref(obj).map_err(heap_error)?;
        Ok(text)
    }

    fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        let parts: Vec<String> = live.iter().map(|(name, n)| format!("{n} {name}")).collect();
        Err(RuntimeError(format!(
            "memory leak: objects were not freed: {}",
            parts.join(", ")
        )))
    }
}

fn internal(what: &str) -> String {
    format!("internal error: {what}")
}

fn heap_error(error: HeapError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_debug_heap_sets_the_flag() {
        assert!(RunConfig::default().with_debug_heap(true).debug_heap);
        assert!(!RunConfig::default().with_debug_heap(false).debug_heap);
    }
}
