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
pub struct DescId(u32);

impl DescId {
    pub const STRING: DescId = DescId(0);
    pub const FRAME: DescId = DescId(1);
}

/// オブジェクトの種類。ヘッダから引けるようにし、後の段階でフィールドのレイアウトと `Lin` の破棄処理を足す
/// (docs/spec/runtime.md の「オブジェクトのヘッダ」)。
#[derive(Debug, Clone)]
pub struct Descriptor {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Str(String),
    Frame(Frame),
}

/// CEK 機械の継続のフレーム。継続もランタイムのオブジェクトにする (docs/spec/runtime.md)。各フィールドの意味は
/// インタプリタが決める。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub function: u32,
    pub resume: u32,
    pub bind: u32,
    /// `None` なら、戻った後も今の環境を使い続ける (入れ子の式のフレーム)。
    pub slots: Option<Vec<Option<Value>>>,
    pub next: Option<ObjRef>,
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
    descriptors: Vec<Descriptor>,
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
            descriptors: vec![
                Descriptor {
                    name: "String".to_string(),
                },
                Descriptor {
                    name: "Frame".to_string(),
                },
            ],
        }
    }

    pub fn register(&mut self, descriptor: Descriptor) -> DescId {
        self.descriptors.push(descriptor);
        DescId(self.descriptors.len() as u32 - 1)
    }

    pub fn alloc(&mut self, desc: DescId, payload: Payload) -> ObjRef {
        let object = Object {
            header: Header {
                rc: AtomicI32::new(1),
                desc,
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

    /// 共有の印付けは、名前だけ予約する。実装はマルチコアの段階で行う (docs/spec/runtime.md)。
    pub fn mark_shared(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        self.object(obj)?;
        Err(HeapError::NotImplemented("mark_shared"))
    }

    /// まだ解放されていないオブジェクトの数を、記述子の名前ごとに数える。`debug_heap` のリーク検出で使う。
    pub fn live_objects(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for object in self.slots.iter().filter_map(|slot| slot.object.as_ref()) {
            let name = &self.descriptors[object.header.desc.0 as usize].name;
            *counts.entry(name.clone()).or_default() += 1;
        }
        counts.into_iter().collect()
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

fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    if let Payload::Frame(frame) = payload {
        work.extend(
            frame
                .slots
                .iter()
                .flatten()
                .flatten()
                .filter_map(|value| match value {
                    Value::Obj(obj) => Some(*obj),
                    _ => None,
                }),
        );
        work.extend(frame.next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(heap: &mut Heap, text: &str) -> ObjRef {
        heap.alloc(DescId::STRING, Payload::Str(text.to_string()))
    }

    fn frame(heap: &mut Heap, slots: Vec<Option<Value>>, next: Option<ObjRef>) -> ObjRef {
        heap.alloc(
            DescId::FRAME,
            Payload::Frame(Frame {
                function: 0,
                resume: 0,
                bind: 0,
                slots: Some(slots),
                next,
            }),
        )
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
        let inner = frame(&mut heap, vec![], None);
        let outer = frame(
            &mut heap,
            vec![Some(Value::Obj(s)), Some(Value::Int(1)), None],
            Some(inner),
        );
        heap.decref(outer).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_long_chain_does_not_overflow_the_stack() {
        let mut heap = Heap::new();
        let mut next = None;
        for _ in 0..200_000 {
            next = Some(frame(&mut heap, vec![], next));
        }
        heap.decref(next.unwrap()).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn live_objects_are_counted_by_descriptor() {
        let mut heap = Heap::new();
        string(&mut heap, "a");
        string(&mut heap, "b");
        frame(&mut heap, vec![], None);
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
    fn mark_shared_is_reserved_for_multicore() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(
            heap.mark_shared(s),
            Err(HeapError::NotImplemented("mark_shared"))
        );
    }

    #[test]
    fn registered_descriptors_are_counted_by_name() {
        let mut heap = Heap::new();
        let desc = heap.register(Descriptor {
            name: "Cell".to_string(),
        });
        heap.alloc(desc, Payload::Str(String::new()));
        assert_eq!(heap.live_objects(), [("Cell".to_string(), 1)]);
    }
}
