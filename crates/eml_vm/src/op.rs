//! VM の命令 (docs/spec/vm.md の「命令の形」)。命令は 8 バイトで、長さの変わるオペランドは `CodeBase::operands` に
//! 置いて添字で指す。

/// フレームの中のスロットの番号。
pub type Slot = u16;
/// `CodeBase::code` の中の命令の位置。
pub type Pc = u32;

/// いちばん下のフレームの戻り先。
pub const FINISH: Pc = 0;
/// 余った引数を適用する組み込みの命令 (Task 5 で中身を入れる)。
#[allow(dead_code)]
pub const APPLY_REST: Pc = 1;
/// 組み込みの命令の数。関数の命令はこの後に並ぶ。
pub const BUILTINS: Pc = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Move {
        dst: Slot,
        src: Slot,
    },
    /// 生の整数 (`int` と `enum` の値)。
    LoadRaw {
        dst: Slot,
        value: i32,
    },
    /// 即値の語 (`#N`、`&f`、`()`)。`()` は値 0 の即値で、生の `unit` の値も同じ語である。
    LoadImm {
        dst: Slot,
        value: i32,
    },
    /// `CodeBase::constants` の語。
    LoadConst {
        dst: Slot,
        index: u32,
    },
    Goto {
        target: Pc,
    },
    Ret {
        src: Slot,
    },
    /// `operands[at..]` は [関数の番号, 引数の数, 引数のスロット…]。
    Call {
        dst: Slot,
        at: u32,
    },
    TailCall {
        at: u32,
    },
    Finish,
    ApplyRest,
    IntAdd {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntSub {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntMul {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntDiv {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntRem {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntNeg {
        dst: Slot,
        a: Slot,
    },
    IntEq {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntNe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntLt {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntLe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntGt {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntGe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    IntCompare {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    BoolEq {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    BoolNe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// 算術と比較以外の extern。`operands[at..]` は [行の番号, 引数の数, 引数のスロット…]。
    Extern {
        dst: Slot,
        at: u32,
    },
}

// 命令の列を密に保ち、1回の読み込みで命令を取れるようにする
const _: () = assert!(std::mem::size_of::<Op>() == 8);
