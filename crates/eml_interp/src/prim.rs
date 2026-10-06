use eml_core_ir::{FALSE, PrimOp, TRUE};
use eml_runtime::{Payload, Value};

use crate::error::Fault;
use crate::machine::Machine;

impl Machine<'_> {
    pub(crate) fn prim(&mut self, op: PrimOp, args: &[Value]) -> Result<Value, Fault> {
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

    /// プリミティブは引数の所有権を受け取るので、読んだ文字列は decref する。
    pub(crate) fn take_string(&mut self, value: Value) -> Result<String, Fault> {
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
}
