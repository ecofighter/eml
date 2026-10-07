use std::io::Read;

use eml_core_ir::{IoOp, TUPLE};
use eml_runtime::{FileHandle, Payload, Value};

use crate::error::{Fault, io_reason};
use crate::machine::Machine;

impl Machine<'_> {
    /// `IO` はユーザーが handle できず、最下部の handler が必ずすぐに再開するので、継続を遡らずにその場で実行する
    /// (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn io(&mut self, op: IoOp, args: &[Value]) -> Result<Value, Fault> {
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
            // 破棄処理はオブジェクトの解放で、読み出し口を捨てると閉じる (docs/spec/runtime.md)
            IoOp::Close => {
                let Value::Obj(file) = args[0] else {
                    return Err(Fault::Internal("`close` on a value that is not a file"));
                };
                self.heap.decref(file).map_err(Fault::Heap)?;
                Ok(Value::Unit)
            }
        }
    }
}
