use eml_core_ir::{FALSE, TRUE};
use eml_extern::Extern;
use eml_runtime::{Payload, Value};

use crate::error::Fault;
use crate::machine::Machine;

impl Machine<'_> {
    /// extern の関数を実行する。既定の腕を置かないので、表に行を足して実装を忘れるとコンパイルが通らない。
    pub(crate) fn call_extern(&mut self, e: Extern, args: &[Value]) -> Result<Value, Fault> {
        let int = |index: usize| match args[index] {
            Value::Int(n) => Ok(n),
            _ => Err(Fault::Internal(
                "an integer operation on a value that is not an integer",
            )),
        };
        let overflow = || Fault::IntegerOverflow;
        let tag = |b: bool| Value::Tag(if b { TRUE } else { FALSE });
        Ok(match e {
            Extern::IntAdd => Value::Int(int(0)?.checked_add(int(1)?).ok_or_else(overflow)?),
            Extern::IntSub => Value::Int(int(0)?.checked_sub(int(1)?).ok_or_else(overflow)?),
            Extern::IntMul => Value::Int(int(0)?.checked_mul(int(1)?).ok_or_else(overflow)?),
            Extern::IntDiv | Extern::IntMod => {
                let (a, b) = (int(0)?, int(1)?);
                if b == 0 {
                    return Err(Fault::DivisionByZero);
                }
                // Rust と同じく 0 の方向に切り捨てる (docs/spec/declarations.md)
                let result = if e == Extern::IntDiv {
                    a.checked_div(b)
                } else {
                    a.checked_rem(b)
                };
                Value::Int(result.ok_or_else(overflow)?)
            }
            Extern::IntNeg => Value::Int(int(0)?.checked_neg().ok_or_else(overflow)?),
            Extern::IntEq => tag(int(0)? == int(1)?),
            Extern::IntNe => tag(int(0)? != int(1)?),
            Extern::IntLt => tag(int(0)? < int(1)?),
            Extern::IntLe => tag(int(0)? <= int(1)?),
            Extern::IntGt => tag(int(0)? > int(1)?),
            Extern::IntGe => tag(int(0)? >= int(1)?),
            Extern::ShowInt => {
                let text = int(0)?.to_string();
                Value::Obj(self.heap.alloc(Payload::Str(text)))
            }
            Extern::StrConcat => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                Value::Obj(self.heap.alloc(Payload::Str(left + &right)))
            }
            // extern は引数の所有権を受け取るので、比べた後に両方の文字列を手放す (`take_string`)
            Extern::StrEq | Extern::StrNe => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                tag((left == right) == (e == Extern::StrEq))
            }
            Extern::BoolEq | Extern::BoolNe => {
                let (Value::Tag(left), Value::Tag(right)) = (args[0], args[1]) else {
                    return Err(Fault::Internal(
                        "a `Bool` comparison on a value that is not a tag",
                    ));
                };
                tag((left == right) == (e == Extern::BoolEq))
            }
            // 型で選ぶ行は translate が比べ方ごとの行に置き換え、verifier が Core IR に残らないことを確かめる
            Extern::Eq | Extern::Ne => {
                return Err(Fault::Internal(
                    "an extern chosen by type reached the interpreter",
                ));
            }
        })
    }

    /// extern と `IO` の操作は引数の所有権を受け取るので、読んだ文字列は decref する。
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
