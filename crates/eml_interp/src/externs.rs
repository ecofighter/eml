use std::io::Read;

use eml_core_ir::{FALSE, TRUE, TUPLE};
use eml_extern::Extern;
use eml_runtime::{FileHandle, ObjRef, Payload, Value};

use crate::error::{Fault, io_reason};
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
            Extern::Println => {
                let text = self.take_string(args[0])?;
                self.out
                    .write_str(&format!("{text}\n"))
                    .map_err(|error| Fault::Output(error.to_string()))?;
                Value::Unit
            }
            Extern::Open => {
                let path = self.take_string(args[0])?;
                // 絶対パスなら `join` がそのパスを返す
                let file = std::fs::File::open(self.file_root.join(&path)).map_err(|error| {
                    Fault::FileOpen {
                        path: path.clone(),
                        reason: io_reason(error.kind()),
                    }
                })?;
                let handle = FileHandle::new(path, Box::new(file));
                Value::Obj(self.heap.alloc(Payload::File(handle)))
            }
            Extern::ReadAll => {
                let Value::Obj(file) = args[0] else {
                    return Err(Fault::Internal("`read_all` on a value that is not a file"));
                };
                self.read_all(file)?
            }
            // 破棄処理はオブジェクトの解放で、読み出し口を捨てると閉じる (docs/spec/runtime.md)
            Extern::Close => {
                let Value::Obj(file) = args[0] else {
                    return Err(Fault::Internal("`close` on a value that is not a file"));
                };
                self.heap.decref(file).map_err(Fault::Heap)?;
                Value::Unit
            }
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

    /// 受け取った `File` の参照を、そのまま返す組に移す (docs/spec/effects.md の「組み込みの `IO`」)。
    fn read_all(&mut self, file: ObjRef) -> Result<Value, Fault> {
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
        let text = match read {
            Ok(text) => text,
            // 実行はここで止まるが、受け取った参照を手放す規律はエラーの経路でも保ち、ファイルをすぐに閉じる
            Err(fault) => {
                self.heap.decref(file).map_err(Fault::Heap)?;
                return Err(fault);
            }
        };
        let text = self.heap.alloc(Payload::Str(text));
        Ok(Value::Obj(self.heap.alloc(Payload::Data {
            tag: TUPLE,
            fields: vec![Value::Obj(file), Value::Obj(text)],
        })))
    }

    /// extern は引数の所有権を受け取るので、読んだ文字列は decref する。
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
