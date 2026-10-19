//! VM の命令 (docs/spec/vm.md の「命令の形」)。命令は 8 バイトで、長さの変わるオペランドは `CodeBase::operands` に
//! 置いて添字で指す。

/// フレームの中のスロットの番号。
pub type Slot = u16;
/// `CodeBase::code` の中の命令の位置。
pub type Pc = u32;

/// いちばん下のフレームの戻り先。
pub const FINISH: Pc = 0;
/// 余った引数を適用する組み込みの命令。
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
    /// `operands[at..]` は [関数の番号, lin, 引数の数, 引数のスロット…]。
    Closure {
        dst: Slot,
        at: u32,
    },
    /// `operands[at..]` は [呼ばれる値のスロット, 引数の数, 引数のスロット…]。
    Apply {
        dst: Slot,
        at: u32,
    },
    TailApply {
        at: u32,
    },
    Clause {
        dst: Slot,
        at: u32,
    },
    TailClause {
        at: u32,
    },
    /// 自己末尾呼び出し。`operands[at..]` は [入口の命令の位置, 引数の数, 引数のスロット…]。
    TailSelf {
        at: u32,
    },
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
    FloatAdd {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatSub {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatMul {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatDiv {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatRem {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatNeg {
        dst: Slot,
        a: Slot,
    },
    FloatEq {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatNe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatLt {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatLe {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatGt {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    FloatGe {
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
    /// タグの switch。scrutinee の Repr で読み方を分ける。語の形では分けない。生の `enum` の値は即値にもポインタにも
    /// 見えるためである (docs/spec/vm.md の「switch」)。
    SwitchEnum {
        src: Slot,
        at: u32,
    },
    SwitchObj {
        src: Slot,
        at: u32,
    },
    SwitchTObj {
        src: Slot,
        at: u32,
    },
    SwitchInt {
        src: Slot,
        at: u32,
    },
    SwitchString {
        src: Slot,
        at: u32,
    },
    Unpack {
        src: Slot,
        at: u32,
    },
    Dup {
        src: Slot,
    },
    Decref {
        src: Slot,
    },
    Release {
        src: Slot,
        at: u32,
    },
    Con {
        dst: Slot,
        at: u32,
    },
    /// token を定義する `release`。`operands[at..]` は [token のスロット, タグ, 走査する語の数, 項目の数, 項目…] で、
    /// 項目は `Release` と同じである。token のスロットは命令に収まらないのでオペランドに置く。
    ReleaseReuse {
        src: Slot,
        at: u32,
    },
    /// token を受ける `con`。`operands[at..]` は [token のスロット, タグ, 走査する語の数, 生の語の数, lin,
    /// in_place, 引数の数, [位置 | RAW | SKIP, スロット]…]。
    ConReuse {
        dst: Slot,
        at: u32,
    },
    DropToken {
        tok: Slot,
    },
    BoxInt {
        dst: Slot,
        src: Slot,
    },
    BoxEnum {
        dst: Slot,
        src: Slot,
    },
    /// `float` の `box`。いつも `Float` の物体を確保する。
    BoxFloat {
        dst: Slot,
        src: Slot,
    },
    /// 定数の `box`。値はコード生成の時点で即値か、不死の `BigInt` か `Float` の物体にしてある。
    BoxConst {
        dst: Slot,
        index: u32,
    },
    /// `int` か `enum` を取り出す `unbox`。即値か `BigInt` の物体を読む。
    Unbox {
        dst: Slot,
        src: Slot,
    },
    /// `float` を取り出す `unbox`。`Float` の物体かダミーを読む。束縛する変数の Repr はコード生成で分かるので、命令を
    /// 分けて、実行時に種類で分けない。
    UnboxFloat {
        dst: Slot,
        src: Slot,
    },
    /// 算術と比較以外の extern。`operands[at..]` は [行の番号, 引数の数, 引数のスロット…]。
    Extern {
        dst: Slot,
        at: u32,
    },
    EvvGet {
        dst: Slot,
    },
    EvvSet {
        dst: Slot,
        l: Slot,
    },
    EvvPush {
        dst: Slot,
        l: Slot,
        e: Slot,
    },
    /// `operands[at..]` は [mask_lists の先頭, 長さ, リストのスロット]。
    EvvMask {
        dst: Slot,
        at: u32,
    },
    /// `operands[at..]` は [エフェクト, 操作の数, 状態のスロット, 外側のスロット]。
    EvNew {
        dst: Slot,
        at: u32,
    },
    /// `operands[at..]` は [evidence のスロット, 操作, 入口のスロット]。
    EvSetClause {
        dst: Slot,
        at: u32,
    },
    EvRenew {
        dst: Slot,
        ev: Slot,
        state: Slot,
    },
    EvLookup {
        dst: Slot,
        effect: u16,
    },
    EvClause {
        dst: Slot,
        ev: Slot,
        op: u16,
    },
    EvEvv {
        dst: Slot,
        ev: Slot,
    },
    EvTakeState {
        dst: Slot,
        ev: Slot,
    },
    EvSetState {
        dst: Slot,
        ev: Slot,
        s: Slot,
    },
    Yielding {
        dst: Slot,
    },
    YieldStatus {
        dst: Slot,
        ev: Slot,
    },
    /// `operands[at..]` は [evidence, 節の本体, κ] のスロット。
    YieldTo {
        dst: Slot,
        at: u32,
    },
    /// 中断は戻らないので、結果のスロットを持たない。
    YieldAbort {
        ev: Slot,
        clause: Slot,
    },
    YieldContTake {
        dst: Slot,
    },
    YieldContSet {
        dst: Slot,
        k: Slot,
    },
    YieldFinish {
        dst: Slot,
    },
    Dummy {
        dst: Slot,
    },
    RootGet {
        dst: Slot,
        root: u32,
    },
    /// `operands[at..]` は [根の番号, 値のスロット]。
    RootSet {
        dst: Slot,
        at: u32,
    },
}

/// 行き先がないことを表すオペランド (switch の `default`、確かめない走査する語の数)。
pub const NONE: u32 = u32::MAX;
/// フィールドの位置のうち、生の語であることを表すビット。
pub const RAW: u32 = 1 << 31;
/// `release` のフィールドの位置のうち、残すフィールドであることを表すビット。
pub const KEEP: u32 = 1 << 31;
/// token を受ける `con` のフィールドの位置のうち、箱を使い回したときに書かないフィールドであることを表すビット。
pub const SKIP: u32 = 1 << 30;

// 命令の列を密に保ち、1回の読み込みで命令を取れるようにする
const _: () = assert!(std::mem::size_of::<Op>() == 8);
