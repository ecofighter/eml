//! 実行するコードの表 `CodeBase` と関数表 `FnEntry` (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。

use std::collections::HashMap;
use std::sync::Arc;

use eml_lir::{FloatBits, Loc};
use eml_rt::{Control, Frozen, HeapError, MaskItem, Mode, Word};

use crate::error::{RuntimeError, SourceLocation};
use crate::op::{BUILTINS, CONVERSION_FRAME_SIZE, CONVERSIONS, Op, Pc, Slot};
use crate::sig::{Class, Scalar, SigId, Signatures};
use crate::vm::{Failure, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnEntry {
    pub name: String,
    /// 命令列の範囲 `[start, end)`。
    pub start: Pc,
    pub end: Pc,
    /// 引数の数。クロージャの捕まえた値も引数に数える。
    pub arity: u16,
    pub frame_size: u16,
    /// 引数と結果の Repr。入口の関数として呼んだとき `Finish` が結果を手放すかも、これで決まる。
    pub(crate) sig: SigId,
    /// `sig` の種類を詰めた語 (`Signatures::fn_key`)。`apply` の速い道が `FnEntry` と一緒に読めるように、ここにも置く。
    pub(crate) key: u64,
}

/// 巻き戻しの地点の表の1行 (docs/spec/vm.md の「巻き戻しの表」)。
/// 地点の命令の次の位置で引く。呼び出しの地点では、それがフレームの見出しの戻り先と一致するためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unwind {
    /// 地点の命令の後で所有している RC の対象のスロットと、その参照の数。命令が束縛する変数は含めない。
    pub(crate) slots: Vec<(Slot, u32)>,
    /// 地点の命令の後で生きている token のスロット。巻き戻しで `drop_token` する。
    pub(crate) tokens: Vec<Slot>,
    /// `apply` の着地の印の evidence のスロット。
    pub(crate) landing: Option<Slot>,
}

/// 変換の命令列。受ける命令の次の `start` から `end` の前までで、`from` の種類の値を受けて `to` の種類で返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Conversion {
    pub(crate) start: Pc,
    pub(crate) end: Pc,
    pub(crate) from: Class,
    pub(crate) to: Class,
}

/// 変換の命令列の表。組の中身で一意にする (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「VM」)。
/// 引くのは変換の要る遅い道だけなので、並びを順に見る。
#[derive(Default)]
pub(crate) struct Conversions {
    list: Vec<Conversion>,
}

impl Conversions {
    /// `from` から `to` への変換の命令列の先頭。同じ種類の間と、違う型の Repr の間には変換がない。
    pub(crate) fn find(&self, from: Class, to: Class) -> Option<Pc> {
        self.list
            .iter()
            .find(|c| (c.from, c.to) == (from, to))
            .map(|c| c.start)
    }

    /// 命令列の先頭が `pc` の変換。フレームの見出しの戻り先と、命令の変換のオペランドを引く。
    pub(crate) fn at(&self, pc: Pc) -> Option<&Conversion> {
        self.list.iter().find(|c| c.start == pc)
    }

    /// `pc` が変換の命令列の中か。
    pub(crate) fn contains(&self, pc: Pc) -> bool {
        self.list.iter().any(|c| c.start <= pc && pc < c.end)
    }
}

/// 組み込みの命令とオペランドと変換の表。`Finish`、`ApplyRest` の後に、スカラーごとに箱へ入れる変換と取り出す変換を
/// 置く。各命令列の直前には受ける命令を置き、受けるスロットは変換のフレームのスロット 0 である。
pub(crate) fn builtins() -> (Vec<Op>, Vec<u32>, Conversions) {
    const RECEIVE: u32 = 0;
    let operands = vec![1, 0];
    let mut code = vec![Op::Finish, Op::ApplyRest];
    let mut conversions = Conversions::default();
    debug_assert_eq!(code.len(), CONVERSIONS as usize);
    for scalar in Scalar::ALL {
        for boxing in [true, false] {
            code.push(Op::Receive { at: RECEIVE });
            let start = code.len() as Pc;
            let (op, from, to) = if boxing {
                (Op::RetBox { src: 0, scalar }, scalar.class(), Class::Word)
            } else {
                (Op::RetUnbox { src: 0, scalar }, Class::Word, scalar.class())
            };
            code.push(op);
            conversions.list.push(Conversion {
                start,
                end: code.len() as Pc,
                from,
                to,
            });
        }
    }
    // 変換のフレームはどれも同じ大きさなので、命令列の使うスロットがその中にあることをここで確かめる
    debug_assert!(
        operands[1..=operands[0] as usize]
            .iter()
            .all(|&slot| (slot as usize) < CONVERSION_FRAME_SIZE)
            && code.iter().all(|op| match op {
                Op::RetBox { src, .. } | Op::RetUnbox { src, .. } =>
                    usize::from(*src) < CONVERSION_FRAME_SIZE,
                _ => true,
            })
    );
    debug_assert_eq!(code.len(), BUILTINS as usize);
    (code, operands, conversions)
}

/// 実行中は読むだけの表。後の並列 VM の worker がそのまま共有できるように、`Sync` に保つ
/// (docs/future/multicore.md の「VM の並列化の備え」の制約2)。入力の境目では `load` で追記する
/// (docs/spec/vm.md の「追記」)。
pub struct CodeBase<M: Mode> {
    pub(crate) functions: Vec<FnEntry>,
    pub(crate) code: Vec<Op>,
    pub(crate) operands: Vec<u32>,
    /// 関数と `apply` の場所のシグネチャ。中身で引くので、追記の間も番号は変わらない。
    pub(crate) signatures: Signatures,
    /// 変換の命令列。
    pub(crate) conversions: Conversions,
    /// `EvvMask` が参照する、mask の項目の並び。
    pub(crate) mask_lists: Vec<MaskItem>,
    pub(crate) constants: Vec<Word>,
    /// 定数を置いた凍結した塊の鎖の先頭。定数の物体をまだ作っていなければ `None`。
    pub(crate) region: Option<Arc<Frozen<M>>>,
    /// 命令の位置の昇順。
    pub(crate) locations: Vec<(Pc, Loc)>,
    /// 巻き戻しの表。地点の命令の次の位置の昇順。
    pub(crate) unwinds: Vec<(Pc, Unwind)>,
    pub(crate) files: Vec<String>,
    /// 追記で番号を引き直す表。
    pub(crate) index: Index,
    /// 定数の塊のページを数える `Control`。実行のヒープと同じものである。
    pub(crate) control: Arc<Control>,
}

/// 名前と中身から `CodeBase` の番号を引く表 (docs/spec/vm.md の「追記」)。
#[derive(Default)]
pub(crate) struct Index {
    pub(crate) functions: HashMap<String, u32>,
    /// エフェクトの名前から実行時の番号。番号は読み込んだ順に振る。
    pub(crate) effects: HashMap<String, u16>,
    pub(crate) files: HashMap<String, u32>,
    /// 文字列の中身から定数の番号。
    pub(crate) strings: HashMap<String, u32>,
    /// 生の整数の値から定数の番号。
    pub(crate) raws: HashMap<i64, u32>,
    /// `tobj` の `Int` の値から定数の番号。
    pub(crate) boxed: HashMap<i64, u32>,
    /// `tobj` の `Float` のビット列から定数の番号。
    pub(crate) boxed_floats: HashMap<FloatBits, u32>,
    /// 不死の空の `MutArray` の定数の番号。最初に使うときに作る。
    pub(crate) empty_mut_array: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CompileError {
    /// 同時に要るスロットが `u16` に収まらない関数。
    TooManySlots { function: String },
    /// 定数を凍結できなかった。
    Freeze(HeapError),
    /// すでにある名前の関数を変換し直すと、前と違うコードになった。debug ビルドだけが確かめる
    /// (docs/spec/vm.md の「追記」)。
    Mismatch { function: String },
}

impl<M: Mode> CodeBase<M> {
    pub fn functions(&self) -> &[FnEntry] {
        &self.functions
    }

    /// 関数の結果が RC の Repr か。
    pub(crate) fn ret_rc(&self, f: u32) -> bool {
        let entry = &self.functions[f as usize];
        self.signatures
            .get(entry.sig)
            .ret
            .iter()
            .any(|repr| repr.is_rc())
    }

    /// `from` から `to` への変換の命令列の先頭。
    pub(crate) fn conversion(&self, from: Class, to: Class) -> Option<Pc> {
        self.conversions.find(from, to)
    }

    /// `pc` が変換の命令列の中か。
    pub(crate) fn is_conversion(&self, pc: Pc) -> bool {
        self.conversions.contains(pc)
    }

    /// 命令の位置を含む関数。組み込みの命令と変換の命令列はどの関数にも属さない。関数は命令の位置の順に並ぶ。関数の
    /// 間に置く変換の命令列を前の関数と取り違えないように、範囲の終わりも比べる。
    pub(crate) fn function_at(&self, pc: Pc) -> Option<&FnEntry> {
        if pc < BUILTINS {
            return None;
        }
        let after = self.functions.partition_point(|f| f.start <= pc);
        self.functions
            .get(after.checked_sub(1)?)
            .filter(|f| pc < f.end)
    }

    pub(crate) fn location(&self, pc: Pc) -> Option<Loc> {
        let index = self
            .locations
            .binary_search_by_key(&pc, |&(at, _)| at)
            .ok()?;
        Some(self.locations[index].1)
    }

    /// 地点の命令の次の位置 `pc` の巻き戻しの表の行。
    pub(crate) fn unwind_at(&self, pc: Pc) -> Option<&Unwind> {
        let index = self.unwinds.binary_search_by_key(&pc, |&(at, _)| at).ok()?;
        Some(&self.unwinds[index].1)
    }

    /// 命令の位置の関数の名前。どの関数にも属さない位置には、タスクの入口の関数の名前を返す。
    pub(crate) fn function_name(&self, pc: Pc, task: &Task) -> String {
        self.function_at(pc)
            .unwrap_or(&self.functions[task.entry as usize])
            .name
            .clone()
    }

    /// 誤りに関数の名前と位置を付ける。組み込みの命令で起きた誤りには、最後に戻った関数の名前を付ける
    /// (docs/spec/vm.md の「誤り、止めること、終わり」)。
    pub(crate) fn error(&self, Failure { fault, pc }: Failure, task: &Task) -> RuntimeError {
        let function = self
            .function_at(pc)
            .or_else(|| self.function_at(task.last_ret))
            .unwrap_or(&self.functions[task.entry as usize])
            .name
            .clone();
        let at = self.location(pc).map(|loc| SourceLocation {
            path: self.files[loc.file as usize].clone(),
            line: loc.line,
            column: loc.column,
        });
        RuntimeError::Fault {
            fault,
            function,
            at,
        }
    }
}
