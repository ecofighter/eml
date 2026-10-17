//! 実行するコードの表 `CodeBase` と関数表 `FnEntry` (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。

use std::collections::HashMap;
use std::sync::Arc;

use eml_lir::Loc;
use eml_rt::{Control, Frozen, HeapError, MaskItem, Mode, Word};

use crate::error::{RuntimeError, SourceLocation};
use crate::op::{BUILTINS, Op, Pc};
use crate::vm::{Failure, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnEntry {
    pub name: String,
    /// 命令列の範囲 `[start, end)`。
    pub start: Pc,
    pub end: Pc,
    pub arity: u16,
    pub frame_size: u16,
    /// `ret` が RC の Repr か。入口の関数として呼んだとき、`Finish` が結果を手放すかを決める。
    pub ret_rc: bool,
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

    /// 命令の位置を含む関数。組み込みの命令はどの関数にも属さない。関数は命令の位置の順に並ぶ。
    pub(crate) fn function_at(&self, pc: Pc) -> Option<&FnEntry> {
        if pc < BUILTINS {
            return None;
        }
        let after = self.functions.partition_point(|f| f.start <= pc);
        self.functions.get(after.checked_sub(1)?)
    }

    pub(crate) fn location(&self, pc: Pc) -> Option<Loc> {
        let index = self
            .locations
            .binary_search_by_key(&pc, |&(at, _)| at)
            .ok()?;
        Some(self.locations[index].1)
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
