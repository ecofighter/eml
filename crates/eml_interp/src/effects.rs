use eml_runtime::{Frame, Link, ObjRef, Payload, Value};

use crate::error::Fault;
use crate::machine::{Machine, Step};

impl Machine<'_> {
    /// 継続の連結リストを先頭から読み、同じエフェクトの一番内側の handler フレームを探す。`Mask` フレームを越えるたびに、
    /// その中の同じエフェクトの数だけ外側の handler を飛ばす (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn find_handler(&self, effect: u32) -> Result<ObjRef, Fault> {
        let mut current = self.cont;
        let mut skip = 0usize;
        loop {
            let Payload::Frame(frame) = self.heap.get(current).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            current = match frame {
                Frame::Handler { effect: other, .. } if *other == effect && skip == 0 => {
                    return Ok(current);
                }
                Frame::Handler {
                    effect: other,
                    link,
                    ..
                } => {
                    if *other == effect {
                        skip -= 1;
                    }
                    link.ok_or(Fault::Internal("a detached handler is in the continuation"))?
                        .next
                }
                Frame::Mask { effects, next } => {
                    skip += effects.iter().filter(|&&masked| masked == effect).count();
                    *next
                }
                Frame::Return { next, .. } | Frame::Apply { next, .. } => *next,
                Frame::Root => return Err(Fault::Internal("an operation without a handler")),
            };
        }
    }

    /// handler フレームの外側を切り離して機械の継続に戻し、節を呼ぶ。先頭から handler フレームまでの区間が継続で、
    /// `once` の操作はそれを継続オブジェクトにして `k` として渡す。`never` の操作は再開しないので、区間をここで
    /// 解放する。区間のフレームが退避した値も、子をたどる解放で1回ずつ解放される。handler フレームの状態は
    /// 切り離した `Link` から取り出し、節の最後の引数として渡す
    /// (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn perform(
        &mut self,
        effect: u32,
        op: u32,
        resumable: bool,
        mut args: Vec<Value>,
    ) -> Result<Step, Fault> {
        let handler = self.find_handler(effect)?;
        let Payload::Frame(Frame::Handler { clauses, link, .. }) =
            self.heap.get_mut(handler).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("a handler that is not a handler frame"));
        };
        let clause = *clauses
            .get(op as usize)
            .ok_or(Fault::Internal("an operation without a clause"))?;
        let Link {
            next: outside,
            state,
        } = link
            .take()
            .ok_or(Fault::Internal("performing through a detached handler"))?;
        // 節のクロージャは handler フレームにも残るので、呼ぶ分の参照を足す
        if let Value::Obj(obj) = clause {
            self.heap.dup(obj).map_err(Fault::Heap)?;
        }
        let top = std::mem::replace(&mut self.cont, outside);
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
    /// 継続をもう一度使うなら継続は共有されていて、`take_or_copy` が区間を写す。どちらの場合も区間のフレームは一意なので、handler フレームを書き換えてよい
    /// (docs/implementation/architecture.md の「継続のフレーム」)。
    pub(crate) fn resume(&mut self, k: Value, value: Value, state: Value) -> Result<Step, Fault> {
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
        let current = self.cont;
        match self.heap.get_mut(handler).map_err(Fault::Heap)? {
            Payload::Frame(Frame::Handler { link, .. }) => {
                *link = Some(Link {
                    next: current,
                    state,
                });
            }
            _ => {
                return Err(Fault::Internal(
                    "a continuation whose handler is not a handler frame",
                ));
            }
        }
        self.cont = top;
        self.ret(value)
    }
}
