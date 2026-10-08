use eml_runtime::{Attachment, Frame, ObjRef, Payload, Value};

use crate::error::Fault;
use crate::runtime::{Runtime, Transfer};

impl Runtime<'_> {
    /// handler の連鎖を先頭から `outer` でたどり、同じエフェクトの一番内側の handler フレームを探す。`Mask` フレームを
    /// 越えるたびに、その中の同じエフェクトの数だけ外側の handler を飛ばす。連鎖は handler と `Mask` のフレームだけで
    /// できていて、`Mask` は handler より多くならない (docs/spec/effects.md の「健全性」) ので、たどる数は handler の
    /// 数に比例する (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn find_handler(&mut self, effect: u32) -> Result<ObjRef, Fault> {
        let mut current = self.handlers;
        let mut skip = 0usize;
        loop {
            self.handler_visits += 1;
            let Payload::Frame(frame) = self.heap.get(current).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the handler chain is not a frame"));
            };
            current = match frame {
                Frame::Handler {
                    effect: other,
                    link,
                    ..
                } => {
                    let Attachment::Attached { outer, .. } = link else {
                        return Err(Fault::Internal(
                            "a detached handler is in the handler chain",
                        ));
                    };
                    if *other == effect {
                        if skip == 0 {
                            return Ok(current);
                        }
                        skip -= 1;
                    }
                    *outer
                }
                Frame::Mask { effects, outer, .. } => {
                    skip += effects.iter().filter(|&&masked| masked == effect).count();
                    *outer
                }
                Frame::Root => return Err(Fault::Internal("an operation without a handler")),
                Frame::Return { .. } | Frame::Apply { .. } => {
                    return Err(Fault::Internal(
                        "the handler chain reaches a frame that is neither a handler nor a mask",
                    ));
                }
            };
        }
    }

    /// handler フレームの外側を切り離して機械の継続に戻し、節を呼ぶ。先頭から handler フレームまでの区間が継続で、
    /// `once` の操作はそれを継続オブジェクトにして `k` として渡す。`never` の操作は再開しないので、区間をここで
    /// 解放する。区間のフレームが退避した値も、子をたどる解放で1回ずつ解放される。handler フレームの状態は
    /// 切り離した `Attachment` から取り出し、節の最後の引数として渡す。連鎖の先頭は handle の外側の連鎖に戻し、
    /// 区間の中の連鎖の先頭は、切り離した handler フレームの `inner` に残す
    /// (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn perform(
        &mut self,
        effect: u32,
        op: u32,
        resumable: bool,
        mut args: Vec<Value>,
    ) -> Result<Transfer, Fault> {
        let handler = self.find_handler(effect)?;
        let Payload::Frame(Frame::Handler { clauses, link, .. }) =
            self.heap.get_mut(handler).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("a handler that is not a handler frame"));
        };
        let clause = *clauses
            .get(op as usize)
            .ok_or(Fault::Internal("an operation without a clause"))?;
        let Attachment::Attached {
            next: outside,
            state,
            outer,
        } = *link
        else {
            return Err(Fault::Internal("performing through a detached handler"));
        };
        // 今の連鎖の先頭は、区間の中でいちばん内側の連鎖のフレームである。区間に連鎖のフレームがなければ handler
        // フレーム自身になる
        *link = Attachment::Detached {
            inner: self.handlers,
        };
        // 節のクロージャは handler フレームにも残るので、呼ぶ分の参照を足す
        if let Value::Obj(obj) = clause {
            self.heap.dup(obj).map_err(Fault::Heap)?;
        }
        let top = std::mem::replace(&mut self.cont, outside);
        self.handlers = outer;
        if resumable {
            let k = self.heap.alloc(Payload::Continuation { top, handler });
            args.push(Value::Obj(k));
        } else {
            self.heap.decref(top).map_err(Fault::Heap)?;
        }
        args.push(state);
        self.apply_and_continue(clause, args)
    }

    /// 継続オブジェクトの handler フレームの外側に今の継続をつなぎ、`state` をその handler フレームの状態に戻して、
    /// 先頭のフレームに値を返す。末尾でない `resume` では、その前に呼び出しのフレームが積まれている。`multi` の
    /// 継続をもう一度使うなら継続は共有されていて、`take_or_copy` が区間を写す。どちらの場合も区間のフレームは
    /// 一意なので、handler フレームを書き換えてよい。handler の連鎖は、handler フレームの `outer` を今の連鎖の先頭に
    /// し、先頭を区間の中の `inner` にしてつなぎ直す。`mask` 付きの `resume` では、その前に積んだ `Mask` フレームが
    /// 今の連鎖の先頭である (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn resume(
        &mut self,
        k: Value,
        value: Value,
        state: Value,
    ) -> Result<Transfer, Fault> {
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
        let attached = Attachment::Attached {
            next: self.cont,
            state,
            outer: self.handlers,
        };
        let inner = match self.heap.get_mut(handler).map_err(Fault::Heap)? {
            Payload::Frame(Frame::Handler { link, .. }) => {
                let Attachment::Detached { inner } = *link else {
                    return Err(Fault::Internal(
                        "resuming a continuation whose handler is attached",
                    ));
                };
                *link = attached;
                inner
            }
            _ => {
                return Err(Fault::Internal(
                    "a continuation whose handler is not a handler frame",
                ));
            }
        };
        if !matches!(
            self.heap.get(inner).map_err(Fault::Heap)?,
            Payload::Frame(Frame::Handler { .. } | Frame::Mask { .. })
        ) {
            return Err(Fault::Internal(
                "the handler chain of a continuation starts at a frame that is neither a handler nor a mask",
            ));
        }
        self.cont = top;
        self.handlers = inner;
        self.ret(value)
    }
}
