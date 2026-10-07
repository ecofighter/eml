//! ヒープと参照カウント (docs/spec/runtime.md)。インデックス方式のアリーナと世代番号で、解放済みのオブジェクトへの
//! アクセスを `unsafe` なしに検出する。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::FileHandle;

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
    /// 捕まえた値のない関数の値。ヒープに置かない。
    Fn(u32),
    Obj(ObjRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DescId(u32);

impl DescId {
    const STRING: DescId = DescId(0);
    const FRAME: DescId = DescId(1);
    const CLOSURE: DescId = DescId(2);
    const CONTINUATION: DescId = DescId(3);
    const DATA: DescId = DescId(4);
    const FILE: DescId = DescId(5);
}

/// オブジェクトの種類。ヘッダから引けるようにし、後の段階で型ごとのフィールドのレイアウトを足す
/// (docs/spec/runtime.md の「オブジェクトのヘッダ」)。`Lin` の破棄処理はオブジェクトの解放で済む。`File` の読み出し口は、解放で捨てると閉じる。
/// `data` のオブジェクトは、型によらず1つの記述子にする。
struct Descriptor {
    name: &'static str,
}

const DESCRIPTORS: [Descriptor; 6] = [
    Descriptor { name: "String" },
    Descriptor { name: "Frame" },
    Descriptor { name: "Closure" },
    Descriptor {
        name: "Continuation",
    },
    Descriptor { name: "Data" },
    Descriptor { name: "File" },
];

#[derive(Debug, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// `perform` で捕まえた継続。先頭のフレーム `top` から `next` をたどった先に `handler` のフレームがある。`top`
    /// だけを所有し、`handler` は所有せずに指す。`handler` の `next` は捕まえられている間 `None` なので、継続を
    /// 解放すると `top` から `handler` までの区間だけが解放される (docs/spec/runtime.md)。共有された継続を写すときは、
    /// 区間のフレームごと写す (`take_or_copy`)。
    Continuation {
        top: ObjRef,
        handler: ObjRef,
    },
    /// 引数を持つコンストラクタの値。フィールドはそれぞれ参照を1つずつ所有する。引数のないコンストラクタの値は
    /// ヒープに置かず、`Value::Tag` にする (docs/spec/runtime.md)。
    Data {
        tag: u32,
        fields: Vec<Value>,
    },
    /// 組み込みの線形型 `File` の値。写さない (`take_or_copy` は `NotCopyable` を返す)。
    File(FileHandle),
}

impl Payload {
    /// 記述子はペイロードの種類から決める。フレームはどの種類も継続の連結リストの要素なので、同じ記述子にする。
    fn desc(&self) -> DescId {
        match self {
            Payload::Str(_) => DescId::STRING,
            Payload::Closure(_) => DescId::CLOSURE,
            Payload::Frame(_) => DescId::FRAME,
            Payload::Continuation { .. } => DescId::CONTINUATION,
            Payload::Data { .. } => DescId::DATA,
            Payload::File(_) => DescId::FILE,
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
    /// `mask` 付きの呼び出しの間、外側の同じエフェクトの handler を飛ばす。`effects` はエフェクトの番号の昇順の多重集合で、
    /// 値を所有しない (docs/implementation/architecture.md の「継続のフレーム」)。
    Mask { effects: Vec<u32>, next: ObjRef },
    /// 継続の最下部にある `IO` の組み込みの handler (docs/implementation/architecture.md の「継続のフレーム」)。
    Io,
    /// handle の handler。節はエフェクトの操作の順に並ぶ。`link` が `None` なのは、継続に捕まえられて handle の
    /// 外側から切り離されている間である (docs/implementation/architecture.md の「継続のフレーム」)。
    Handler {
        effect: u32,
        clauses: Vec<Value>,
        ret: Value,
        link: Option<Link>,
    },
}

/// つながっている handler フレームの外側と状態。切り離すと状態は節に渡るので、2つを一緒に持つ
/// (docs/spec/runtime.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub next: ObjRef,
    pub state: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    Shared,
    /// 継続の区間が、切り離された handler フレームで終わっていない。
    BrokenSegment,
    NotImplemented(&'static str),
    NotCopyable,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::Shared => f.write_str("an object is still shared"),
            HeapError::BrokenSegment => {
                f.write_str("a continuation does not end at a detached handler")
            }
            HeapError::NotImplemented(name) => write!(f, "`{name}` is not implemented yet"),
            HeapError::NotCopyable => f.write_str("a file cannot be copied"),
        }
    }
}

struct Header {
    /// 正なら局所、負なら共有を表す。今は常に正である (docs/spec/runtime.md)。
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
    /// 数えるので、写すときと解放するときで数える参照が一致する。継続オブジェクトは区間のフレームごと写す。フレームを
    /// 共有させないためである (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if self.is_unique(obj)? {
            return self.take(obj);
        }
        // `File` は線形で、型検査が共有させない。共有されていたら処理系の誤りなので、写さずに止める
        if matches!(self.object(obj)?.payload, Payload::File(_)) {
            return Err(HeapError::NotCopyable);
        }
        let segment = match &self.object(obj)?.payload {
            Payload::Continuation { top, .. } => Some(*top),
            _ => None,
        };
        let copy = match segment {
            Some(top) => {
                let (top, handler) = self.copy_segment(top)?;
                Payload::Continuation { top, handler }
            }
            None => {
                let copy = copy(&self.object(obj)?.payload);
                let mut shared = Vec::new();
                children(&copy, &mut shared);
                for child in shared {
                    self.dup(child)?;
                }
                copy
            }
        };
        self.decref(obj)?;
        Ok(copy)
    }

    /// 継続の区間を、先頭のフレームから切り離された handler フレーム (`next` が `None`) まで写し、写した区間の先頭と
    /// handler フレームを返す。写したフレームは `next` 以外の子の参照を1つずつ増やし、`next` は写した次のフレームを
    /// 指す。元のフレームの参照の数は変えないので、写した後も両方の区間のフレームは一意である。長い区間で Rust の
    /// スタックを溢れさせないよう、ループでたどる (docs/spec/runtime.md)。
    fn copy_segment(&mut self, top: ObjRef) -> Result<(ObjRef, ObjRef), HeapError> {
        let mut frames = Vec::new();
        let mut current = top;
        loop {
            frames.push(current);
            current = match &self.object(current)?.payload {
                Payload::Frame(Frame::Handler { link: None, .. }) => break,
                Payload::Frame(
                    Frame::Return { next, .. }
                    | Frame::Apply { next, .. }
                    | Frame::Mask { next, .. }
                    | Frame::Handler {
                        link: Some(Link { next, .. }),
                        ..
                    },
                ) => *next,
                _ => return Err(HeapError::BrokenSegment),
            };
        }
        // 下から写し、写した次のフレームを `next` に入れる
        let mut below = None;
        let mut handler = None;
        for &frame in frames.iter().rev() {
            let mut payload = copy(&self.object(frame)?.payload);
            set_next(&mut payload, below)?;
            let mut shared = Vec::new();
            children(&payload, &mut shared);
            // 写した次のフレームは、この写しだけが所有する
            for child in shared.into_iter().filter(|&child| Some(child) != below) {
                self.dup(child)?;
            }
            let copied = self.alloc(payload);
            handler.get_or_insert(copied);
            below = Some(copied);
        }
        match (below, handler) {
            (Some(top), Some(handler)) => Ok((top, handler)),
            _ => Err(HeapError::BrokenSegment),
        }
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

/// 中身の写し。子の参照は数え直さないので、`take_or_copy` と `copy_segment` だけが使う。継続オブジェクトは区間ごと
/// 写すので、ここでは扱わない。
fn copy(payload: &Payload) -> Payload {
    match payload {
        Payload::Str(text) => Payload::Str(text.clone()),
        Payload::Data { tag, fields } => Payload::Data {
            tag: *tag,
            fields: fields.clone(),
        },
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
        Payload::Frame(Frame::Mask { effects, next }) => Payload::Frame(Frame::Mask {
            effects: effects.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Io) => Payload::Frame(Frame::Io),
        Payload::Frame(Frame::Handler {
            effect,
            clauses,
            ret,
            link,
        }) => Payload::Frame(Frame::Handler {
            effect: *effect,
            clauses: clauses.clone(),
            ret: *ret,
            link: *link,
        }),
        Payload::Continuation { .. } => {
            unreachable!("a shared continuation is copied with its segment by `copy_segment`")
        }
        Payload::File(_) => unreachable!("`take_or_copy` refuses to copy a file"),
    }
}

/// 写したフレームの次を、写した次のフレームにする。切り離された handler フレームだけが次を持たない。
fn set_next(payload: &mut Payload, below: Option<ObjRef>) -> Result<(), HeapError> {
    match payload {
        Payload::Frame(
            Frame::Return { next, .. } | Frame::Apply { next, .. } | Frame::Mask { next, .. },
        ) => {
            *next = below.ok_or(HeapError::BrokenSegment)?;
        }
        Payload::Frame(Frame::Handler { link, .. }) => match (link, below) {
            (Some(link), Some(below)) => link.next = below,
            (None, None) => {}
            _ => return Err(HeapError::BrokenSegment),
        },
        _ => return Err(HeapError::BrokenSegment),
    }
    Ok(())
}

/// 子のオブジェクト。解放と、共有されたオブジェクトの複製 (`take_or_copy`) が、同じ子を数える。
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
        Payload::Frame(Frame::Mask { effects: _, next }) => work.push(*next),
        Payload::Frame(Frame::Handler {
            effect: _,
            clauses,
            ret,
            link,
        }) => {
            work.extend(clauses.iter().filter_map(object));
            work.extend(object(ret));
            if let Some(Link { next, state }) = link {
                work.extend(object(state));
                work.push(*next);
            }
        }
        // `handler` は所有しない。`top` からたどれる
        Payload::Continuation { top, .. } => work.push(*top),
        Payload::Closure(closure) => work.extend(closure.args.iter().filter_map(object)),
        Payload::Data { fields, .. } => work.extend(fields.iter().filter_map(object)),
        Payload::Frame(Frame::Io) | Payload::Str(_) | Payload::File(_) => {}
    }
}

#[cfg(test)]
mod tests;
