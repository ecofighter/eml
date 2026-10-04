//! ヒープと参照カウント (docs/spec/runtime.md)。インデックス方式のアリーナと世代番号で、解放済みのオブジェクトへの
//! アクセスを `unsafe` なしに検出する。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjRef {
    index: u32,
    generation: u32,
}

/// インタプリタの値。`Copy` で、`Rc` も `RefCell` も使わない (docs/spec/core-ir.md の「実行時の規約」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Unit,
    /// 引数のないコンストラクタ。`Bool` は `False` = 0、`True` = 1 である。
    Tag(u32),
    Obj(ObjRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DescId(u32);

impl DescId {
    const STRING: DescId = DescId(0);
    const FRAME: DescId = DescId(1);
    const CLOSURE: DescId = DescId(2);
    const CONTINUATION: DescId = DescId(3);
}

/// オブジェクトの種類。ヘッダから引けるようにし、後の段階でフィールドのレイアウトと `Lin` の破棄処理を足す
/// (docs/spec/runtime.md の「オブジェクトのヘッダ」)。段階4で、ユーザーの `data` の記述子を登録できるようにする。
struct Descriptor {
    name: &'static str,
}

const DESCRIPTORS: [Descriptor; 4] = [
    Descriptor { name: "String" },
    Descriptor { name: "Frame" },
    Descriptor { name: "Closure" },
    Descriptor {
        name: "Continuation",
    },
];

#[derive(Debug, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// `perform` で捕まえた継続。先頭のフレーム `top` から `next` をたどった先に `handler` のフレームがある。`top`
    /// だけを所有し、`handler` は所有せずに指す。`handler` の `next` は捕まえられている間 `None` なので、継続を
    /// 解放すると `top` から `handler` までの区間だけが解放される (docs/spec/runtime.md)。
    Continuation {
        top: ObjRef,
        handler: ObjRef,
    },
}

impl Payload {
    /// 記述子はペイロードの種類から決める。フレームはどの種類も継続の連結リストの要素なので、同じ記述子にする。
    fn desc(&self) -> DescId {
        match self {
            Payload::Str(_) => DescId::STRING,
            Payload::Closure(_) => DescId::CLOSURE,
            Payload::Frame(_) => DescId::FRAME,
            Payload::Continuation { .. } => DescId::CONTINUATION,
        }
    }
}

/// クロージャ。関数と、すでに渡された先頭の引数の並び (docs/spec/core-ir.md)。各値は参照を1つずつ所有する。
#[derive(Debug, PartialEq)]
pub struct Closure {
    pub function: u32,
    pub args: Vec<Value>,
}

/// CEK 機械の継続のフレーム。継続もランタイムのオブジェクトにする (docs/spec/runtime.md)。
#[derive(Debug, PartialEq)]
pub enum Frame {
    /// 呼び出し元の関数に戻る。呼び出しの後で使う変数だけを退避し、それぞれ参照を1つ所有する
    /// (docs/spec/core-ir.md)。
    Return {
        function: u32,
        resume: u32,
        bind: u32,
        saved: Vec<(u32, Value)>,
        next: ObjRef,
    },
    /// 戻った関数値に、余った引数を適用する (docs/spec/core-ir.md の eval/apply)。
    Apply { args: Vec<Value>, next: ObjRef },
    /// 継続の最下部にある `IO` の組み込みの handler (docs/spec/core-ir.md)。
    Io,
    /// handle の handler。節のクロージャはエフェクトの操作の順に並ぶ。`next` が `None` なのは、継続に捕まえられて
    /// handle の外側から切り離されている間である (docs/spec/core-ir.md)。
    Handler {
        effect: u32,
        clauses: Vec<Value>,
        ret: Option<Value>,
        next: Option<ObjRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    Shared,
    NotImplemented(&'static str),
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::Shared => f.write_str("an object is still shared"),
            HeapError::NotImplemented(name) => write!(f, "`{name}` is not implemented yet"),
        }
    }
}

struct Header {
    /// 正なら局所、負なら共有を表す。マイルストーン1 では常に正である (docs/spec/runtime.md)。
    rc: AtomicI32,
    desc: DescId,
}

struct Object {
    header: Header,
    payload: Payload,
}

struct Slot {
    generation: u32,
    object: Option<Object>,
}

pub struct Heap {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn alloc(&mut self, payload: Payload) -> ObjRef {
        let object = Object {
            header: Header {
                rc: AtomicI32::new(1),
                desc: payload.desc(),
            },
            payload,
        };
        match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.object = Some(object);
                ObjRef {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    object: Some(object),
                });
                ObjRef {
                    index: self.slots.len() as u32 - 1,
                    generation: 0,
                }
            }
        }
    }

    pub fn get(&self, obj: ObjRef) -> Result<&Payload, HeapError> {
        Ok(&self.object(obj)?.payload)
    }

    pub fn get_mut(&mut self, obj: ObjRef) -> Result<&mut Payload, HeapError> {
        Ok(&mut self.object_mut(obj)?.payload)
    }

    pub fn dup(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        *self.object_mut(obj)?.header.rc.get_mut() += 1;
        Ok(())
    }

    pub fn is_unique(&self, obj: ObjRef) -> Result<bool, HeapError> {
        Ok(self.object(obj)?.header.rc.load(Ordering::Relaxed) == 1)
    }

    /// 子は再帰ではなく作業リストでたどる。長い連鎖の解放で Rust のスタックを溢れさせないため (docs/spec/runtime.md)。
    pub fn decref(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        let mut work = vec![obj];
        while let Some(obj) = work.pop() {
            let remaining = {
                let rc = self.object_mut(obj)?.header.rc.get_mut();
                *rc -= 1;
                *rc
            };
            if remaining == 0 {
                let payload = self.release(obj);
                children(&payload, &mut work);
            }
        }
        Ok(())
    }

    /// 一意なオブジェクトを解放して中身を返す。子の所有権は呼び出し側に移る。
    pub fn take(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if !self.is_unique(obj)? {
            return Err(HeapError::Shared);
        }
        Ok(self.release(obj))
    }

    /// オブジェクトの所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されていれば
    /// 中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ `children` で
    /// 数えるので、写すときと解放するときで数える参照が一致する (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if self.is_unique(obj)? {
            return self.take(obj);
        }
        let copy = copy(&self.object(obj)?.payload);
        let mut shared = Vec::new();
        children(&copy, &mut shared);
        for child in shared {
            self.dup(child)?;
        }
        self.decref(obj)?;
        Ok(copy)
    }

    /// 共有の印付けは、名前だけ予約する。実装はマルチコアの段階で行う (docs/spec/runtime.md)。
    pub fn mark_shared(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        self.object(obj)?;
        Err(HeapError::NotImplemented("mark_shared"))
    }

    /// まだ解放されていないオブジェクトの数を、記述子の名前ごとに数える。`debug_heap` のリーク検出で使う。
    pub fn live_objects(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for object in self.slots.iter().filter_map(|slot| slot.object.as_ref()) {
            *counts
                .entry(DESCRIPTORS[object.header.desc.0 as usize].name)
                .or_default() += 1;
        }
        counts
            .into_iter()
            .map(|(name, count)| (name.to_string(), count))
            .collect()
    }

    fn object(&self, obj: ObjRef) -> Result<&Object, HeapError> {
        self.slots
            .get(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_ref())
            .ok_or(HeapError::UseAfterFree)
    }

    fn object_mut(&mut self, obj: ObjRef) -> Result<&mut Object, HeapError> {
        self.slots
            .get_mut(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_mut())
            .ok_or(HeapError::UseAfterFree)
    }

    /// スロットを空けて世代番号を進める。古い `ObjRef` は、以後の検査で解放済みとして見つかる。
    fn release(&mut self, obj: ObjRef) -> Payload {
        let slot = &mut self.slots[obj.index as usize];
        let object = slot
            .object
            .take()
            .expect("the caller checked the reference");
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(obj.index);
        object.payload
    }
}

/// 中身の写し。子の参照は数え直さないので、`take_or_copy` だけが使う。
fn copy(payload: &Payload) -> Payload {
    match payload {
        Payload::Str(text) => Payload::Str(text.clone()),
        Payload::Closure(closure) => Payload::Closure(Closure {
            function: closure.function,
            args: closure.args.clone(),
        }),
        Payload::Frame(Frame::Return {
            function,
            resume,
            bind,
            saved,
            next,
        }) => Payload::Frame(Frame::Return {
            function: *function,
            resume: *resume,
            bind: *bind,
            saved: saved.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Apply { args, next }) => Payload::Frame(Frame::Apply {
            args: args.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Io) => Payload::Frame(Frame::Io),
        Payload::Frame(Frame::Handler {
            effect,
            clauses,
            ret,
            next,
        }) => Payload::Frame(Frame::Handler {
            effect: *effect,
            clauses: clauses.clone(),
            ret: *ret,
            next: *next,
        }),
        Payload::Continuation { top, handler } => Payload::Continuation {
            top: *top,
            handler: *handler,
        },
    }
}

/// 子のオブジェクト。解放と、段階3以降の複製が、同じ子を数える。
fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    let object = |value: &Value| match value {
        Value::Obj(obj) => Some(*obj),
        _ => None,
    };
    match payload {
        // 退避した値はそれぞれ参照を1つ所有するので、1回ずつ解放する
        Payload::Frame(Frame::Return { saved, next, .. }) => {
            work.extend(saved.iter().filter_map(|(_, value)| object(value)));
            work.push(*next);
        }
        Payload::Frame(Frame::Apply { args, next }) => {
            work.extend(args.iter().filter_map(object));
            work.push(*next);
        }
        Payload::Frame(Frame::Handler {
            clauses, ret, next, ..
        }) => {
            work.extend(clauses.iter().filter_map(object));
            work.extend(ret.as_ref().and_then(object));
            work.extend(*next);
        }
        // `handler` は所有しない。`top` からたどれる
        Payload::Continuation { top, .. } => work.push(*top),
        Payload::Closure(closure) => work.extend(closure.args.iter().filter_map(object)),
        Payload::Frame(Frame::Io) | Payload::Str(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(heap: &mut Heap, text: &str) -> ObjRef {
        heap.alloc(Payload::Str(text.to_string()))
    }

    /// 継続の最下部のフレーム。
    fn bottom(heap: &mut Heap) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Io))
    }

    fn frame(heap: &mut Heap, saved: Vec<(u32, Value)>, next: ObjRef) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Return {
            function: 0,
            resume: 0,
            bind: 0,
            saved,
            next,
        }))
    }

    fn handler(
        heap: &mut Heap,
        clauses: Vec<Value>,
        ret: Option<Value>,
        next: Option<ObjRef>,
    ) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Handler {
            effect: 1,
            clauses,
            ret,
            next,
        }))
    }

    #[test]
    fn a_handler_frame_releases_its_clauses_and_the_rest_of_the_continuation() {
        let mut heap = Heap::new();
        let clause = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![],
        }));
        let ret = heap.alloc(Payload::Closure(Closure {
            function: 1,
            args: vec![],
        }));
        let end = bottom(&mut heap);
        let frame = handler(
            &mut heap,
            vec![Value::Obj(clause)],
            Some(Value::Obj(ret)),
            Some(end),
        );
        heap.decref(frame).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_continuation_stops_at_its_detached_handler() {
        let mut heap = Heap::new();
        // handler の外側は機械の継続が持っている
        let outside = bottom(&mut heap);
        let s = string(&mut heap, "saved");
        let detached = handler(&mut heap, vec![], None, None);
        let top = frame(&mut heap, vec![(0, Value::Obj(s))], detached);
        let k = heap.alloc(Payload::Continuation {
            top,
            handler: detached,
        });
        assert_eq!(
            heap.live_objects(),
            vec![
                ("Continuation".to_string(), 1),
                ("Frame".to_string(), 3),
                ("String".to_string(), 1),
            ]
        );
        heap.decref(k).unwrap();
        assert_eq!(heap.live_objects(), vec![("Frame".to_string(), 1)]);
        heap.decref(outside).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn a_closure_releases_its_arguments() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let closure = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Obj(s), Value::Int(1)],
        }));
        heap.decref(closure).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn an_apply_frame_releases_its_arguments_and_the_rest_of_the_continuation() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let end = bottom(&mut heap);
        let next = frame(&mut heap, vec![], end);
        let apply = heap.alloc(Payload::Frame(Frame::Apply {
            args: vec![Value::Obj(s)],
            next,
        }));
        heap.decref(apply).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn decref_to_zero_frees_the_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
        assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    }

    #[test]
    fn dup_keeps_the_object_alive() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        heap.decref(s).unwrap();
        assert_eq!(heap.get(s).unwrap(), &Payload::Str("a".to_string()));
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn a_reused_slot_does_not_revive_old_references() {
        let mut heap = Heap::new();
        let old = string(&mut heap, "a");
        heap.decref(old).unwrap();
        let new = string(&mut heap, "b");
        assert_ne!(old, new);
        assert_eq!(heap.get(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.get(new).unwrap(), &Payload::Str("b".to_string()));
    }

    #[test]
    fn decref_releases_children() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let inner = bottom(&mut heap);
        let outer = frame(
            &mut heap,
            vec![(0, Value::Obj(s)), (1, Value::Int(1))],
            inner,
        );
        heap.decref(outer).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_long_chain_does_not_overflow_the_stack() {
        let mut heap = Heap::new();
        let mut next = bottom(&mut heap);
        for _ in 0..200_000 {
            next = frame(&mut heap, vec![], next);
        }
        heap.decref(next).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn live_objects_are_counted_by_descriptor() {
        let mut heap = Heap::new();
        string(&mut heap, "a");
        string(&mut heap, "b");
        bottom(&mut heap);
        assert_eq!(
            heap.live_objects(),
            [("Frame".to_string(), 1), ("String".to_string(), 2)]
        );
    }

    #[test]
    fn take_requires_a_unique_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        assert_eq!(heap.take(s), Err(HeapError::Shared));
        heap.decref(s).unwrap();
        assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn take_or_copy_takes_a_unique_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("a".to_string())));
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn take_or_copy_copies_a_shared_object_and_its_children() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let closure = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Obj(s), Value::Int(1)],
        }));
        heap.dup(closure).unwrap();
        let copy = heap.take_or_copy(closure).unwrap();
        assert_eq!(
            copy,
            Payload::Closure(Closure {
                function: 0,
                args: vec![Value::Obj(s), Value::Int(1)],
            })
        );
        // 元のクロージャと写した中身が、捕まえた文字列の参照を1つずつ持つ
        heap.decref(closure).unwrap();
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn mark_shared_is_reserved_for_multicore() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(
            heap.mark_shared(s),
            Err(HeapError::NotImplemented("mark_shared"))
        );
    }
}
