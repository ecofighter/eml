//! ヒープと参照カウント (docs/spec/runtime.md)。インデックス方式のアリーナと世代番号で、解放済みのオブジェクトへの
//! アクセスを `unsafe` なしに検出する。

use std::collections::{BTreeMap, HashMap};
use std::fmt;

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

/// オブジェクトの中身。enum の形がレイアウトで、`Lin` の値の破棄処理はオブジェクトの解放で済む (docs/spec/runtime.md)。
#[derive(Debug, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// `perform` で捕まえた継続。先頭のフレーム `top` から `next` をたどった先に `handler` のフレームがある。`top`
    /// だけを所有し、`handler` は所有せずに指す。`handler` は捕まえられている間 `Attachment::Detached` で `next` を
    /// 持たないので、継続を解放すると `top` から `handler` までの区間だけが解放される (docs/spec/runtime.md)。共有
    /// された継続を写すときは、区間のフレームごと写す (`take_or_copy`)。
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
    /// `debug_heap` がリークを数えるときの種類の名前。フレームはどの種類も継続の連結リストの要素なので、1つの名前にする。
    /// `data` のオブジェクトも、型によらず1つの名前にする。
    pub fn kind_name(&self) -> &'static str {
        match self {
            Payload::Str(_) => "String",
            Payload::Closure(_) => "Closure",
            Payload::Frame(_) => "Frame",
            Payload::Continuation { .. } => "Continuation",
            Payload::Data { .. } => "Data",
            Payload::File(_) => "File",
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
    /// (docs/spec/core-ir.md)。`resume` は再開の番地、`saved` の鍵はスロットの番号で、どちらも実行する側が意味を
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
    /// `mask` 付きの呼び出しの間、外側の同じエフェクトの handler を飛ばす。`effects` はエフェクトの番号の昇順の多重集合で、
    /// 値を所有しない。`outer` は handler の連鎖で外側の次のフレーム (handler、`Mask`、`Root` のどれか) を指し、所有しない
    /// (docs/spec/runtime.md の「handler の連鎖」)。
    Mask {
        effects: Vec<u32>,
        next: ObjRef,
        outer: ObjRef,
    },
    /// 継続の最下部。ここへ戻ればプログラムが終わる。handler ではないので、操作の handler を探してここに届いたら内部の
    /// 誤りである (docs/implementation/architecture.md の「継続のフレーム」)。
    Root,
    /// handle の handler。節はエフェクトの操作の順に並ぶ (docs/spec/core-ir.md の「Core IR」)。
    Handler {
        effect: u32,
        clauses: Vec<Value>,
        ret: Value,
        link: Attachment,
    },
}

/// handler フレームと handle の外側のつながり。`perform` は handler と `Mask` のフレームだけを `outer` でたどるので、
/// 費用は継続の深さではなく handler の数に比例する (docs/spec/runtime.md の「handler の連鎖」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attachment {
    /// handle の外側につながっている。`next` (外側の継続) と `state` は参照を1つずつ所有する。`outer` は handler の
    /// 連鎖で外側の次のフレームを指し、所有しない。切り離すと状態は節に渡るので、3つを一緒に持つ。
    Attached {
        next: ObjRef,
        state: Value,
        outer: ObjRef,
    },
    /// 継続に捕まえられて、handle の外側から切り離されている。`inner` は捕まえた区間の中でいちばん内側の連鎖の
    /// フレームで、区間に連鎖のフレームがなければこの handler 自身を指す。所有しない。`resume` は、区間をたどらずに
    /// `inner` から連鎖をつなぎ直す。
    Detached { inner: ObjRef },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    Shared,
    /// 継続の区間が切り離された handler フレームで終わっていないか、区間の中の handler の連鎖が区間の外を指す。
    BrokenSegment,
    /// `take_or_copy` に、クロージャと継続のほかの物体が渡された。
    NotCopyable,
    /// 文字列の操作に、文字列でない物体が渡された。
    NotAString,
    /// `release_fields` に渡した物体が、名指したタグとフィールドの数の `data` でない。
    WrongLayout,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::Shared => f.write_str("an object is still shared"),
            HeapError::BrokenSegment => f.write_str("a continuation segment is broken"),
            HeapError::NotCopyable => f.write_str("an object cannot be copied"),
            HeapError::NotAString => f.write_str("an object is not a string"),
            HeapError::WrongLayout => {
                f.write_str("an object does not have the tag and number of fields a release names")
            }
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

    pub fn dup(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        self.object_mut(obj)?.header.rc += 1;
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

    /// クロージャか継続の所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されて
    /// いれば中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ
    /// `children` で数えるので、写すときと解放するときで数える参照が一致する。継続オブジェクトは区間のフレームごと
    /// 写す。フレームを共有させないためである。クロージャと継続のほかの物体は、一意かどうかによらず
    /// `NotCopyable` で断り、参照を手放さない。`data` は写さずに `release_fields` で分解し、`File` は線形である
    /// (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        let segment = match &self.object(obj)?.payload {
            Payload::Continuation { top, .. } => Some(*top),
            Payload::Closure(_) => None,
            _ => return Err(HeapError::NotCopyable),
        };
        if self.is_unique(obj)? {
            return self.take(obj);
        }
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

    /// 継続の区間を、先頭のフレームから切り離された handler フレームまで写し、写した区間の先頭と handler フレームを
    /// 返す。写したフレームは `next` 以外の子の参照を1つずつ増やし、`next` は写した次のフレームを指す。元のフレームの
    /// 参照の数は変えないので、写した後も両方の区間のフレームは一意である。handler の連鎖の参照 (`outer` と `inner`)
    /// は所有しないので数えず、元のフレームから写したフレームへの対応で付け替える。長い区間で Rust のスタックを
    /// 溢れさせないよう、ループでたどる (docs/spec/runtime.md)。
    fn copy_segment(&mut self, top: ObjRef) -> Result<(ObjRef, ObjRef), HeapError> {
        let frames = self.segment_frames(top)?;
        let mut copies = HashMap::with_capacity(frames.len());
        // 下から写し、写した次のフレームを `next` に入れる
        let mut below = None;
        for &frame in frames.iter().rev() {
            let mut payload = copy(&self.object(frame)?.payload);
            set_next(&mut payload, below)?;
            // `outer` は区間の下の方のフレームを指すので、もう写してある。h の `inner` は上の方を指すので、すべて
            // 写した後で付け替える
            if below.is_some()
                && let Some(outer) = chain_link(&mut payload)
            {
                *outer = remap(&copies, *outer)?;
            }
            let mut shared = Vec::new();
            children(&payload, &mut shared);
            // 写した次のフレームは、この写しだけが所有する
            for child in shared.into_iter().filter(|&child| Some(child) != below) {
                self.dup(child)?;
            }
            let copied = self.alloc(payload);
            copies.insert(frame, copied);
            below = Some(copied);
        }
        let top = below.ok_or(HeapError::BrokenSegment)?;
        let handler = remap(&copies, *frames.last().ok_or(HeapError::BrokenSegment)?)?;
        let inner = chain_link(self.get_mut(handler)?).ok_or(HeapError::BrokenSegment)?;
        *inner = remap(&copies, *inner)?;
        Ok((top, handler))
    }

    /// 区間のフレームを、先頭から切り離された handler フレーム h まで並べる。たどるついでに、区間の中の handler の
    /// 連鎖が h の `inner` から `outer` で h までつながり、区間の外を指さないことを確かめる。区間をすべてたどるのは
    /// 写すときだけなので、検査の費用は写す費用に含まれる (docs/spec/runtime.md の「handler の連鎖」)。
    fn segment_frames(&self, top: ObjRef) -> Result<Vec<ObjRef>, HeapError> {
        let mut frames = Vec::new();
        // 区間の中でいちばん内側の連鎖のフレームと、直前の連鎖のフレームの `outer`
        let mut innermost = None;
        let mut expected = None;
        let mut current = top;
        loop {
            frames.push(current);
            let (next, outer) = match &self.object(current)?.payload {
                Payload::Frame(Frame::Handler {
                    link: Attachment::Detached { inner },
                    ..
                }) => {
                    if expected.is_some_and(|outer| outer != current)
                        || *inner != innermost.unwrap_or(current)
                    {
                        return Err(HeapError::BrokenSegment);
                    }
                    return Ok(frames);
                }
                Payload::Frame(
                    Frame::Mask { next, outer, .. }
                    | Frame::Handler {
                        link: Attachment::Attached { next, outer, .. },
                        ..
                    },
                ) => (*next, Some(*outer)),
                Payload::Frame(Frame::Return { next, .. } | Frame::Apply { next, .. }) => {
                    (*next, None)
                }
                _ => return Err(HeapError::BrokenSegment),
            };
            if let Some(outer) = outer {
                if expected.is_some_and(|expected| expected != current) {
                    return Err(HeapError::BrokenSegment);
                }
                innermost.get_or_insert(current);
                expected = Some(outer);
            }
            current = next;
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

/// クロージャとフレームの中身の写し。子の参照は数え直さないので、`take_or_copy` (クロージャ) と `copy_segment`
/// (区間のフレーム) だけが使う。継続オブジェクトは区間ごと写し、データ、文字列、`File` は写さないので、ここでは
/// 扱わない (docs/spec/runtime.md)。
fn copy(payload: &Payload) -> Payload {
    match payload {
        Payload::Closure(closure) => Payload::Closure(Closure {
            function: closure.function,
            args: closure.args.clone(),
        }),
        Payload::Frame(Frame::Return {
            function,
            resume,
            saved,
            next,
        }) => Payload::Frame(Frame::Return {
            function: *function,
            resume: *resume,
            saved: saved.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Apply { args, next }) => Payload::Frame(Frame::Apply {
            args: args.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Mask {
            effects,
            next,
            outer,
        }) => Payload::Frame(Frame::Mask {
            effects: effects.clone(),
            next: *next,
            outer: *outer,
        }),
        Payload::Frame(Frame::Root) => Payload::Frame(Frame::Root),
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
        Payload::Str(_) | Payload::Data { .. } | Payload::File(_) => {
            unreachable!("`take_or_copy` copies only closures and continuations")
        }
    }
}

/// 写したフレームの次を、写した次のフレームにする。切り離された handler フレームだけが次を持たない。
fn set_next(payload: &mut Payload, below: Option<ObjRef>) -> Result<(), HeapError> {
    match (payload, below) {
        (
            Payload::Frame(
                Frame::Return { next, .. }
                | Frame::Apply { next, .. }
                | Frame::Mask { next, .. }
                | Frame::Handler {
                    link: Attachment::Attached { next, .. },
                    ..
                },
            ),
            Some(below),
        ) => *next = below,
        (
            Payload::Frame(Frame::Handler {
                link: Attachment::Detached { .. },
                ..
            }),
            None,
        ) => {}
        _ => return Err(HeapError::BrokenSegment),
    }
    Ok(())
}

/// handler の連鎖の参照。`Mask` とつながった handler フレームは `outer`、切り離された handler フレームは `inner`
/// である。どれも所有しない。
fn chain_link(payload: &mut Payload) -> Option<&mut ObjRef> {
    match payload {
        Payload::Frame(
            Frame::Mask { outer, .. }
            | Frame::Handler {
                link: Attachment::Attached { outer, .. },
                ..
            },
        ) => Some(outer),
        Payload::Frame(Frame::Handler {
            link: Attachment::Detached { inner },
            ..
        }) => Some(inner),
        _ => None,
    }
}

/// 元の区間のフレームを、写した区間のフレームに対応させる。対応にない参照は区間の外を指している。
fn remap(copies: &HashMap<ObjRef, ObjRef>, frame: ObjRef) -> Result<ObjRef, HeapError> {
    copies.get(&frame).copied().ok_or(HeapError::BrokenSegment)
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
        // handler の連鎖の `outer` と `inner` は所有しない
        Payload::Frame(Frame::Mask {
            effects: _,
            next,
            outer: _,
        }) => work.push(*next),
        Payload::Frame(Frame::Handler {
            effect: _,
            clauses,
            ret,
            link,
        }) => {
            work.extend(clauses.iter().filter_map(object));
            work.extend(object(ret));
            match link {
                Attachment::Attached {
                    next,
                    state,
                    outer: _,
                } => {
                    work.extend(object(state));
                    work.push(*next);
                }
                Attachment::Detached { inner: _ } => {}
            }
        }
        // `handler` は所有しない。`top` からたどれる
        Payload::Continuation { top, .. } => work.push(*top),
        Payload::Closure(closure) => work.extend(closure.args.iter().filter_map(object)),
        Payload::Data { fields, .. } => work.extend(fields.iter().filter_map(object)),
        Payload::Frame(Frame::Root) | Payload::Str(_) | Payload::File(_) => {}
    }
}

#[cfg(test)]
mod tests;
