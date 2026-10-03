use std::fmt;
use std::sync::Arc;

use eml_core_ir::{Atom, CExpr, CExprId, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId};
use eml_runtime::{DescId, Frame, Heap, HeapError, ObjRef, OutputSink, Payload, Value};

/// 将来 `threads` などを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
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
    slots: Vec<Option<Value>>,
    /// 継続の先頭のフレーム。最下部には常に `IO` の handler のフレームがある。
    cont: ObjRef,
}

impl<'p> Machine<'p> {
    fn new(program: &'p Program, out: &'p OutputSink) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(
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
        let mut slots = vec![None; main.vars.len()];
        // `main : Unit -> <IO> Unit` の引数
        for param in &main.params {
            slots[param.0 as usize] = Some(Value::Unit);
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
            CExpr::Let { var, rhs, body } => self.bind(*var, rhs, *body)?,
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
            CExpr::Dup { var, body } => {
                if let Value::Obj(obj) = self.peek(*var)? {
                    self.heap.dup(obj).map_err(heap_error)?;
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

    fn bind(&mut self, var: VarId, rhs: &Rhs, body: CExprId) -> Result<(), String> {
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
            Rhs::CallDirect(callee, args) => {
                let args = self.atoms(args)?;
                self.push_frame(var, body, true);
                let target = self.program.function(*callee);
                let mut slots = vec![None; target.vars.len()];
                for (param, value) in target.params.iter().zip(args) {
                    slots[param.0 as usize] = Some(value);
                }
                self.slots = slots;
                self.function = *callee;
                self.control = target.body;
                return Ok(());
            }
            Rhs::Nested(inner) => {
                self.push_frame(var, body, false);
                self.control = *inner;
                return Ok(());
            }
        };
        self.slots[var.0 as usize] = Some(value);
        self.control = body;
        Ok(())
    }

    /// 呼び出しでは環境ごと退避する。入れ子の式は同じ関数の中なので、環境をそのまま使い続ける。
    fn push_frame(&mut self, bind: VarId, resume: CExprId, save_env: bool) {
        let slots = save_env.then(|| std::mem::take(&mut self.slots));
        let frame = Frame {
            function: self.function.0,
            resume: resume.0,
            bind: bind.0,
            slots,
            next: Some(self.cont),
        };
        self.cont = self.heap.alloc(DescId::FRAME, Payload::Frame(frame));
    }

    /// 継続の先頭のフレームに値を返す。最下部の `IO` の handler に届いたら、プログラムが終わる。
    fn ret(&mut self, value: Value) -> Result<bool, String> {
        // 段階1では継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う
        let Payload::Frame(frame) = self.heap.take(self.cont).map_err(heap_error)? else {
            return Err(internal("the continuation is not a frame"));
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
        self.slots[frame.bind as usize] = Some(value);
        self.cont = frame
            .next
            .ok_or_else(|| internal("a frame without a next frame"))?;
        Ok(false)
    }

    /// ヒープの値の読み出しは所有権の移動だが、誰が所有するかは Perceus が静的に決めて `dup` / `decref` を挿入済みである。
    /// `dup s; return s` や `s ++ s` のように、複製した後に同じ変数を読むことがあるので、読んでもスロットは空にしない
    /// (docs/spec/core-ir.md)。参照カウントの収支は `dup` / `decref` と、プリミティブの消費だけが動かす。
    fn atom(&self, atom: &Atom) -> Result<Value, String> {
        Ok(match *atom {
            Atom::Var(var) => self.peek(var)?,
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
        })
    }

    fn atoms(&self, atoms: &[Atom]) -> Result<Vec<Value>, String> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    fn peek(&self, var: VarId) -> Result<Value, String> {
        self.slots[var.0 as usize].ok_or_else(|| internal("a variable read after it was moved"))
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
            Payload::Frame(_) => {
                return Err(internal("a string operation on a frame"));
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
