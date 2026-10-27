//! 実行するコードの表 `CodeBase` と関数表 `FnEntry` (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。

use std::collections::HashMap;
use std::sync::Arc;

use eml_lir::{FloatBits, Loc, Repr};
use eml_rt::{Control, Frozen, HeapError, MaskItem, Mode, Word};

use crate::error::{RuntimeError, SourceLocation};
use crate::op::{BUILTINS, Op, Pc, Slot};
use crate::vm::{Failure, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnEntry {
    pub name: String,
    /// 命令列の範囲 `[start, end)`。
    pub start: Pc,
    pub end: Pc,
    /// 引数の語の数。クロージャの捕まえた値も引数に数える。
    pub arity: u16,
    pub frame_size: u16,
    /// 引数の語の Repr。捕まえた値はクロージャの中で Repr のとおり走査する語か生の語に分かれるので、クロージャを
    /// 作るときと入るときに語の並べ方を引く (docs/spec/object-model.md の「クロージャ」)。
    pub(crate) params: Box<[Repr]>,
    /// 結果が RC の Repr か。入口の関数として呼んだとき、`Finish` が結果を手放すかを決める。
    pub(crate) ret_rc: bool,
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

/// 組み込みの命令 `Finish`、`ApplyRest`、`ChildDone`。関数の命令はこの後に並ぶ。
pub(crate) fn builtins() -> Vec<Op> {
    let code = vec![Op::Finish, Op::ApplyRest, Op::ChildDone];
    debug_assert_eq!(code.len(), BUILTINS as usize);
    code
}

/// 実行中は読むだけの表。後の並列 VM の worker がそのまま共有できるように、`Sync` に保つ
/// (docs/future/multicore.md の「VM の並列化の備え」の制約2)。入力の境目では `load` で追記する
/// (docs/spec/vm.md の「追記」)。
pub struct CodeBase<M: Mode> {
    pub(crate) functions: Vec<FnEntry>,
    pub(crate) code: Vec<Op>,
    pub(crate) operands: Vec<u32>,
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
    /// `par` の子の `evv` を作る、凍結した fork の表と末尾。
    pub(crate) forks: Forks,
}

/// `par` の子の `evv` の凍結した部分 (docs/spec/vm.md の「`par` の fork と join」)。ロードごとに、新しく `fork$` の名前が
/// 引けるようになったエフェクトの表を、定数と同じ凍結した塊に足す。
pub(crate) struct Forks {
    /// 実行時のエフェクトの番号ごとの表。読み込んだエフェクトの数だけ並ぶ。
    pub(crate) tables: Vec<ForkTable>,
    /// `Panic` の実行時の番号。`Panic` の evidence は末尾の1つが受け持つので、fork のたびには作らない。
    pub(crate) panic: Option<u16>,
    /// 子の `evv` の末尾。`Panic` の fork evidence の節点で、`Panic` の表がまだなければ `()` である。
    pub(crate) tail: Word,
}

/// 1つのエフェクトの fork の表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForkTable {
    /// 再開する操作を持つ。`par` の子はこのエフェクトを使えないので、子の `evv` に写さない。
    Resumable,
    /// never-only だが、`fork$` の名前がまだ全部は引けない。
    Missing,
    /// 凍結した表 `Data(tag 0)`。欄は操作ごとの即値 `&fork$E.op` である。
    Table(Word),
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
    /// 結果が複数の値の入口の関数。入口の `ret` は値1つで、`Finish` は値を1つだけ受ける
    /// (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。
    MultiValueEntry { function: String },
    /// `par` を走らせうるプログラムで、`fork$` の関数が名前で引けない。evidence passing はそのプログラムに fork の
    /// 関数をすべて作るので、正しいプログラムでは起きない (docs/spec/vm.md の「`par` の fork と join」)。
    MissingFork { function: String },
}

impl<M: Mode> CodeBase<M> {
    pub fn functions(&self) -> &[FnEntry] {
        &self.functions
    }

    /// 命令の位置を含む関数。組み込みの命令はどの関数にも属さない。関数は命令の位置の順に並ぶ。
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
