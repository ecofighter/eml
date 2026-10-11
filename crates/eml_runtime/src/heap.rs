//! ヒープと参照カウント (docs/spec/runtime.md)。インデックス方式のアリーナと世代番号で、解放済みのオブジェクトへの
//! アクセスを `unsafe` なしに検出する。

use std::collections::BTreeMap;
use std::fmt;

use crate::FileHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjRef {
    index: u32,
    generation: u32,
}

/// インタプリタの値。`Copy` で、`Rc` も `RefCell` も使わない (docs/spec/lir.md の「実行時の規約」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Unit,
    /// 引数のないコンストラクタ。`Bool` は `False` = 0、`True` = 1 である。
    Tag(u32),
    /// 捕まえた値のない関数の値。ヒープに置かない。
    Fn(u32),
    Obj(ObjRef),
    /// yield 中の関数が返す、呼び出し元が捨てる値。ヒープに置かないので、`dup` と `decref` は何もせず、`box` と
    /// `unbox` もそのまま通す (docs/spec/lir.md の「ダミーの値」)。
    Dummy,
}

/// オブジェクトの中身。enum の形がレイアウトで、`Lin` の値の破棄処理はオブジェクトの解放で済む (docs/spec/runtime.md)。
#[derive(Debug, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// 引数を持つコンストラクタの値。フィールドはそれぞれ参照を1つずつ所有する。引数のないコンストラクタの値は
    /// ヒープに置かず、`Value::Tag` にする (docs/spec/runtime.md)。
    Data {
        tag: u32,
        fields: Vec<Value>,
    },
    /// 組み込みの線形型 `File` の値。写さない (`take_or_copy` は `NotCopyable` を返す)。
    File(FileHandle),
    /// evidence の入口の表。`entries` は操作の順の入口のクロージャで、handle の場所で入れた後は変えない。再開で
    /// 作り直した evidence もこの表を共有する (docs/spec/runtime.md の
    /// 「evidence と `evv` の物体」)。
    EvTable {
        effect: u32,
        entries: Vec<Value>,
    },
    /// handler の1回の実行。`outer` は作ったときの `evv` で、この evidence 自身を含まない。書き換えるのは `state`
    /// だけである。yield の宛先はこの物体の参照そのもので比べる。
    Evidence {
        table: ObjRef,
        state: Value,
        outer: Value,
    },
    /// `evv` のリストの節点。リストは書き換えない永続リストで、空のリストは `Value::Unit` である。
    Evv {
        evidence: ObjRef,
        next: Value,
    },
}

impl Payload {
    /// `debug_heap` がリークを数えるときの種類の名前。フレームはどの種類も継続の連結リストの要素なので、1つの名前にする。
    /// `data` のオブジェクトも、型によらず1つの名前にする。
    pub fn kind_name(&self) -> &'static str {
        match self {
            Payload::Str(_) => "String",
            Payload::Closure(_) => "Closure",
            Payload::Frame(_) => "Frame",
            Payload::Data { .. } => "Data",
            Payload::File(_) => "File",
            Payload::EvTable { .. } => "EvTable",
            Payload::Evidence { .. } => "Evidence",
            Payload::Evv { .. } => "Evv",
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
    /// (docs/spec/lir.md)。`resume` は再開の番地、`saved` の鍵はスロットの番号で、どちらも実行する側が意味を
    /// 決める。ランタイムはその中身を解釈しない。番地を `u64` にしたのは、実行する側が自分の制御の位置 (将来の
    /// バイトコード VM なら `pc`) をそのまま入れられるようにするためである。
    Return {
        function: u32,
        resume: u64,
        saved: Vec<(u32, Value)>,
        next: ObjRef,
    },
    /// 戻った関数値に、余った引数を適用する (docs/spec/core-ir.md の eval/apply)。
    Apply { args: Vec<Value>, next: ObjRef },
    /// 継続の最下部。ここへ戻ればプログラムが終わる。
    Root,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    Shared,
    /// `take_or_copy` に、クロージャのほかの物体が渡された。
    NotCopyable,
    /// 文字列の操作に、文字列でない物体が渡された。
    NotAString,
    /// `release_fields` に渡した物体が、名指したタグとフィールドの数の `data` でない。
    WrongLayout,
    /// `Lin` の物体 (`File`) の参照を増やそうとした。`Lin` の物体の RC は 1 である
    /// (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「`File` の実行時の検査」)。
    LinShared,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::Shared => f.write_str("an object is still shared"),
            HeapError::NotCopyable => f.write_str("an object cannot be copied"),
            HeapError::NotAString => f.write_str("an object is not a string"),
            HeapError::WrongLayout => {
                f.write_str("an object does not have the tag and number of fields a release names")
            }
            HeapError::LinShared => f.write_str("a Lin object is shared"),
        }
    }
}

/// 普通の物体の `rc` は1以上で、0 になったら解放する。不死の物体 (`immortal`) は解放せず、`rc` は外に出ている参照の
/// 数である。数が 0 の間は、`acquire_immortal` のほかの操作を断る (docs/spec/runtime.md)。
struct Header {
    rc: u32,
    immortal: bool,
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
    /// 文字列の物体の中身を書くのはヒープの手続き (確保と連結) なので、ヒープが数える。インタプリタはこれを
    /// `RunStats` の `string_bytes_copied` として返す。
    string_bytes_written: u64,
    /// 参照の数を書き換えた回数。増やす側 (`dup`、`acquire_immortal`) と減らす側 (`decref`、連鎖を含む) を分けて
    /// 数える。数を書くのはこの3つの手続きだけなので、ここで数えれば漏れない。箱の解放そのものは数を書かないので
    /// 数えない。ただし `release_fields` は、一意な箱を数を書かずに空けるときも、箱の参照を1つ手放したことを減らす
    /// 側に1回数える。インタプリタはこれを `RunStats` の `rc_increments` と `rc_decrements` として返す。
    rc_increments: u64,
    rc_decrements: u64,
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
            string_bytes_written: 0,
            rc_increments: 0,
            rc_decrements: 0,
        }
    }

    pub fn string_bytes_written(&self) -> u64 {
        self.string_bytes_written
    }

    pub fn rc_increments(&self) -> u64 {
        self.rc_increments
    }

    pub fn rc_decrements(&self) -> u64 {
        self.rc_decrements
    }

    /// 同時に生きていた物体の数の最大。フレームと不死のリテラルも数える。`insert` は空いたスロットを先に使い、
    /// 空きがないときだけスロットを足すので、スロットの数がそのまま最大になる。インタプリタはこれを `RunStats` の
    /// `peak_objects` として返す。
    pub fn peak_objects(&self) -> u64 {
        self.slots.len() as u64
    }

    pub fn alloc(&mut self, payload: Payload) -> ObjRef {
        if let Payload::Str(text) = &payload {
            self.string_bytes_written += text.len() as u64;
        }
        self.insert(Object {
            header: Header {
                rc: 1,
                immortal: false,
            },
            payload,
        })
    }

    /// 不死の物体を作る。数は 0 で始まり、`acquire_immortal` が参照を1つずつ作る。文字列のリテラルの表から1回だけ
    /// 作るので、中身のバイトは実行の仕事として数えない (docs/spec/runtime.md)。
    pub fn alloc_immortal(&mut self, payload: Payload) -> ObjRef {
        self.insert(Object {
            header: Header {
                rc: 0,
                immortal: true,
            },
            payload,
        })
    }

    /// 不死の物体の参照を1つ作る (`Rhs::ConstString`)。数が 0 の物体を使えるのはこの操作だけなので、`object` の
    /// 判定を通らずにスロットを引く。
    pub fn acquire_immortal(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        let object = self.slot_object_mut(obj).ok_or(HeapError::UseAfterFree)?;
        debug_assert!(
            object.header.immortal,
            "`acquire_immortal` takes an immortal object"
        );
        object.header.rc += 1;
        self.rc_increments += 1;
        Ok(())
    }

    pub fn get(&self, obj: ObjRef) -> Result<&Payload, HeapError> {
        Ok(&self.object(obj)?.payload)
    }

    pub fn get_mut(&mut self, obj: ObjRef) -> Result<&mut Payload, HeapError> {
        Ok(&mut self.object_mut(obj)?.payload)
    }

    /// `File` は線形で、参照が2つになると閉じ忘れや二重の解放になる。コンパイラが `dup` を出す誤りを実行時に見つけるため、
    /// 数を変えずに断る (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「`File` の実行時の検査」)。
    pub fn dup(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        let object = self.object_mut(obj)?;
        if matches!(object.payload, Payload::File(_)) {
            return Err(HeapError::LinShared);
        }
        object.header.rc += 1;
        self.rc_increments += 1;
        Ok(())
    }

    /// 不死の物体は一意にならない。中身を書き換えたり取り出したりすると、同じリテラルのほかの評価に見えるため
    /// (docs/spec/runtime.md)。
    pub fn is_unique(&self, obj: ObjRef) -> Result<bool, HeapError> {
        let header = &self.object(obj)?.header;
        Ok(header.rc == 1 && !header.immortal)
    }

    /// 子は再帰ではなく作業リストでたどる。長い連鎖の解放で Rust のスタックを溢れさせないため (docs/spec/runtime.md)。
    /// 不死の物体は数が 0 になっても解放しない。
    pub fn decref(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        self.decref_all(vec![obj])
    }

    /// `release_fields` が、残さないフィールドを集めたリストをそのまま作業リストにして手放せるように、`decref` から
    /// 分けてある。
    fn decref_all(&mut self, mut work: Vec<ObjRef>) -> Result<(), HeapError> {
        while let Some(obj) = work.pop() {
            let header = &mut self.object_mut(obj)?.header;
            header.rc -= 1;
            let freed = header.rc == 0 && !header.immortal;
            self.rc_decrements += 1;
            if freed {
                let payload = self.free_slot(obj);
                children(&payload, &mut work);
            }
        }
        Ok(())
    }

    /// 一意な左辺の文字列の後に右辺の文字列を足し、足したバイト数を返す。スロットを解放して取り直さないので、左辺の
    /// `ObjRef` はそのまま使える。左辺の中身をいったん取り出してから右辺を借りる。左辺と右辺が同じ物体なら、
    /// 取り出した後の右辺が空に見えて何も足さずに終わるので、RC によらず先に `Shared` で断る。RC が 1 のまま
    /// 同じ参照を両辺に渡すのは数え誤りである。共有された物体や不死の物体も一意にならないので `Shared` になる
    /// (docs/spec/runtime.md の「文字列の連結」)。
    pub fn append_str(&mut self, left: ObjRef, right: ObjRef) -> Result<usize, HeapError> {
        if left == right {
            return Err(HeapError::Shared);
        }
        if !self.is_unique(left)? {
            return Err(HeapError::Shared);
        }
        let Payload::Str(text) = &mut self.object_mut(left)?.payload else {
            return Err(HeapError::NotAString);
        };
        let mut text = std::mem::take(text);
        let appended = self.str(right).map(|tail| {
            text.push_str(tail);
            tail.len()
        });
        // 右辺が文字列でなくても、取り出した中身を左辺に戻してから誤りを返す
        if let Payload::Str(slot) = &mut self.object_mut(left)?.payload {
            *slot = text;
        }
        let appended = appended?;
        self.string_bytes_written += appended as u64;
        Ok(appended)
    }

    fn str(&self, obj: ObjRef) -> Result<&str, HeapError> {
        match &self.object(obj)?.payload {
            Payload::Str(text) => Ok(text),
            _ => Err(HeapError::NotAString),
        }
    }

    /// 一意なオブジェクトを解放して中身を返す。子の所有権は呼び出し側に移る。不死の物体は一意にならないので、
    /// `Shared` で断る。
    pub fn take(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if !self.is_unique(obj)? {
            return Err(HeapError::Shared);
        }
        Ok(self.free_slot(obj))
    }

    /// クロージャの所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されていれば中身を
    /// 写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ `children` で数えるので、
    /// 写すときと解放するときで数える参照が一致する。クロージャのほかの物体は、一意かどうかによらず `NotCopyable` で
    /// 断り、参照を手放さない。`data` は写さずに `release_fields` で分解し、`File` は線形である
    /// (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        let Payload::Closure(closure) = &self.object(obj)?.payload else {
            return Err(HeapError::NotCopyable);
        };
        if self.is_unique(obj)? {
            return self.take(obj);
        }
        // evidence の物体は写さない。クロージャが捕まえた evidence は、参照を増やして共有する
        let copy = Payload::Closure(Closure {
            function: closure.function,
            args: closure.args.clone(),
        });
        let mut shared = Vec::new();
        children(&copy, &mut shared);
        for child in shared {
            self.dup(child)?;
        }
        self.decref(obj)?;
        Ok(copy)
    }

    /// `data` の物体の参照を1つ手放し、`keep[i]` が真のフィールドの参照を1つずつ呼び出し側に渡す。一意な箱は箱だけを
    /// 解放し、残さないフィールドの参照を手放す。残すフィールドは箱の参照をそのまま受け継ぐ。共有された箱と不死の
    /// 箱は、残すフィールドを `dup` してから箱の参照を1つ手放す。物体の形が名指しと違えば、何も変えずに
    /// `WrongLayout` を返す (docs/spec/runtime.md の「ランタイムの API」)。
    pub fn release_fields(
        &mut self,
        obj: ObjRef,
        tag: u32,
        keep: &[bool],
    ) -> Result<(), HeapError> {
        let Payload::Data { tag: found, fields } = &self.object(obj)?.payload else {
            return Err(HeapError::WrongLayout);
        };
        if *found != tag || fields.len() != keep.len() {
            return Err(HeapError::WrongLayout);
        }
        if self.is_unique(obj)? {
            let Payload::Data { fields, .. } = self.free_slot(obj) else {
                unreachable!("the layout was checked above");
            };
            self.rc_decrements += 1;
            // 参照の数を動かすのは残さないフィールドだけである。手放すフィールドを1つの作業リストにまとめ、解放の連鎖に
            // そのまま渡す
            let dropped = fields
                .into_iter()
                .zip(keep)
                .filter_map(|(value, &kept)| match value {
                    Value::Obj(field) if !kept => Some(field),
                    _ => None,
                })
                .collect();
            self.decref_all(dropped)
        } else {
            // 参照の数を動かすのは残すフィールドだけである。`dup` がヒープを書き換えるので、フィールドを借りたままにはできない。
            // 集めて確保する代わりに、`dup` のたびに読み直す
            for (index, &kept) in keep.iter().enumerate() {
                let Payload::Data { fields, .. } = &self.object(obj)?.payload else {
                    unreachable!("the layout was checked above");
                };
                if let (Value::Obj(field), true) = (fields[index], kept) {
                    self.dup(field)?;
                }
            }
            self.decref(obj)
        }
    }

    /// まだ解放されていないオブジェクトの数を、種類の名前 (`Payload::kind_name`) ごとに数える。`debug_heap` のリーク
    /// 検出で使う。不死の物体は解放しないので、外に出ている参照の数を足す。数が 0 なら手放し忘れはないので、何も
    /// 足さない (docs/spec/runtime.md)。
    pub fn live_objects(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for object in self.slots.iter().filter_map(|slot| slot.object.as_ref()) {
            let count = if object.header.immortal {
                object.header.rc as usize
            } else {
                1
            };
            if count > 0 {
                *counts.entry(object.payload.kind_name()).or_default() += count;
            }
        }
        counts
            .into_iter()
            .map(|(name, count)| (name.to_string(), count))
            .collect()
    }

    /// 普通の物体は数が 0 になると解放するので、スロットに残る数 0 の物体は不死の物体だけである。その物体の使用も、
    /// 解放済みの物体の使用と同じ誤りにする。判定はここと `object_mut` の2か所だけに置く。
    fn object(&self, obj: ObjRef) -> Result<&Object, HeapError> {
        self.slots
            .get(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_ref())
            .filter(|object| object.header.rc > 0)
            .ok_or(HeapError::UseAfterFree)
    }

    fn object_mut(&mut self, obj: ObjRef) -> Result<&mut Object, HeapError> {
        self.slot_object_mut(obj)
            .filter(|object| object.header.rc > 0)
            .ok_or(HeapError::UseAfterFree)
    }

    /// 世代番号だけを確かめてスロットの物体を引く。数を見ないので、`object_mut` と `acquire_immortal` のほかは使わない。
    fn slot_object_mut(&mut self, obj: ObjRef) -> Option<&mut Object> {
        self.slots
            .get_mut(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_mut())
    }

    /// スロットに物体を置く。空いたスロットがあれば使い、その世代番号の参照を返す。
    fn insert(&mut self, object: Object) -> ObjRef {
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

    /// スロットを空けて世代番号を進める。古い `ObjRef` は、以後の検査で解放済みとして見つかる。
    fn free_slot(&mut self, obj: ObjRef) -> Payload {
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

/// 子のオブジェクト。解放と、共有されたオブジェクトの複製 (`take_or_copy`) が、同じ子を数える。
fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    let object = |value: &Value| match value {
        Value::Obj(obj) => Some(*obj),
        _ => None,
    };
    match payload {
        // 退避した値はそれぞれ参照を1つ所有するので、1回ずつ解放する
        Payload::Frame(Frame::Return {
            function: _,
            resume: _,
            saved,
            next,
        }) => {
            work.extend(saved.iter().filter_map(|(_, value)| object(value)));
            work.push(*next);
        }
        Payload::Frame(Frame::Apply { args, next }) => {
            work.extend(args.iter().filter_map(object));
            work.push(*next);
        }
        Payload::Closure(closure) => work.extend(closure.args.iter().filter_map(object)),
        Payload::Data { fields, .. } => work.extend(fields.iter().filter_map(object)),
        Payload::EvTable { effect: _, entries } => work.extend(entries.iter().filter_map(object)),
        Payload::Evidence {
            table,
            state,
            outer,
        } => {
            work.push(*table);
            work.extend(object(state));
            work.extend(object(outer));
        }
        Payload::Evv { evidence, next } => {
            work.push(*evidence);
            work.extend(object(next));
        }
        Payload::Frame(Frame::Root) | Payload::Str(_) | Payload::File(_) => {}
    }
}

#[cfg(test)]
mod tests;
