//! VM の命令 (docs/spec/vm.md の「命令の形」)。命令は 8 バイトで、長さの変わるオペランドは `CodeBase::operands` に
//! 置いて添字で指す。

use crate::sig::Scalar;

/// フレームの中のスロットの番号。
pub type Slot = u16;
/// `CodeBase::code` の中の命令の位置。
pub type Pc = u32;

/// いちばん下のフレームの戻り先。
pub const FINISH: Pc = 0;
/// 余った引数を適用する組み込みの命令。そのフレームは [結果, n, 場所のシグネチャ, 残りの引数の最初の位置, 引数…] で、
/// 引数は場所のシグネチャの Repr のまま持つ (docs/spec/vm.md の「フレーム」)。
pub const APPLY_REST: Pc = 1;
/// 箱を要するスカラーと一様な値の間の変換の命令列の、最初の位置。命令列は `CodeBase::new` が作る
/// (docs/spec/vm.md の「変換のフレーム」)。
pub const CONVERSIONS: Pc = 2;
/// 組み込みの命令の数。関数の命令はこの後に並ぶ。変換の命令列は、受ける命令と変換して戻る命令の2つを6組持つ。
pub const BUILTINS: Pc = CONVERSIONS + 2 * 6;
/// スカラーの変換のフレームの大きさ。変換のフレームの大きさは変換ごとに受ける値の数で、構造体の箱へ入れる変換は葉の
/// 数のスロットを持つ。合成は恒等か、違う型の間の誤りにしかならないので、変換のフレームを別の大きさの変換に置き換える
/// ことはない (`vm::tail_convert`)。命令列を作るときに、使うスロットがこの中にあることを確かめる。
pub const CONVERSION_FRAME_SIZE: usize = 1;

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
    /// `operands[at..]` は [関数の番号, 引数の数, 引数のスロット…, 結果の変換]。結果の変換は変換の命令列の位置か
    /// `NONE` である。
    TailCall {
        at: u32,
    },
    /// 複数の値を返す直接の呼び出し。`operands[at..]` は [関数の番号, 引数の数, 引数のスロット…, 結果の数,
    /// 結果のスロット…]。結果は呼ばれる側の `RetN` が、見出しの戻り先の直前のこの命令から読んだスロットに書く。
    CallN {
        at: u32,
    },
    /// 複数の値の戻り。`operands[at..]` は [値の数, 値のスロット…]。
    RetN {
        at: u32,
    },
    Finish,
    ApplyRest,
    /// 変換の命令列の先頭の直前に置く、受けるスロットの命令。`operands[at..]` は [値の数, スロット…]。実行はしない。
    /// 戻りが値を書くスロットを、戻り先の直前の命令から読めるようにするためにある。
    Receive {
        at: u32,
    },
    /// 変換のフレームの本体。`src` のスカラーを箱に入れて呼び出し元へ返す。
    RetBox {
        src: Slot,
        scalar: Scalar,
    },
    /// 変換のフレームの本体。`src` の箱からスカラーを取り出し、箱を手放して呼び出し元へ返す。
    RetUnbox {
        src: Slot,
        scalar: Scalar,
    },
    /// 変換のフレームの本体。スロット 0.. の葉から構造体 `id` の箱を作って呼び出し元へ返す。
    RetBoxStruct {
        id: u32,
    },
    /// 変換のフレームの本体。スロット 0 の箱から構造体 `id` の葉を取り出し、箱を消費して呼び出し元へ返す。
    RetUnboxStruct {
        id: u32,
    },
    /// `operands[at..]` は [関数の番号, lin, 捕まえる源の引数の数, 引数の数, 引数のスロット…]。源の引数の数は
    /// 構造体の引数を1つと数える。`&f` の即値にはこの数を詰める。
    Closure {
        dst: Slot,
        at: u32,
    },
    /// `operands[at..]` は [呼ばれる値のスロット, 引数の数, 場所のシグネチャ, 引数のスロット…]。
    Apply {
        dst: Slot,
        at: u32,
    },
    TailApply {
        at: u32,
    },
    /// 複数の値を受ける `apply`。`operands[at..]` は [呼ばれる値のスロット, 引数の数, 場所のシグネチャ,
    /// 引数のスロット…, 結果の数, 結果のスロット…]。
    ApplyN {
        at: u32,
    },
    /// `operands[at..]` は [呼ばれる値のスロット, 引数の数, 引数のスロット…]。引数は操作の ABI の Repr である。
    Clause {
        dst: Slot,
        at: u32,
    },
    /// `operands[at..]` は [呼ばれる値のスロット, 引数の数, 引数のスロット…, 結果の変換]。
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
    /// 構造体の箱 `src` のフィールドを読む。`operands[at..]` は `Unpack` と同じ [タグ, 走査する語の数, 項目の数,
    /// [位置 | RAW, スロット]…] である。箱がダミーなら、各スロットに葉のダミーを書く。
    UnboxStruct {
        src: Slot,
        at: u32,
    },
    /// 構造体の `unbox` で葉を借りた箱の `release`。`operands[at..]` は [token のスロットか NONE, タグ,
    /// 走査する語の数, 項目の数, 項目…] で、項目は `Release` と同じである。箱がダミーなら何もせず、token の形は空の
    /// token を書く。ふつうの `release` の速い道に確かめを足さないため、命令を分ける
    /// (docs/spec/vm.md の「データとクロージャ」)。
    ReleaseUnboxed {
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
    /// `Std.Array.get` と `Std.MutArray.read`。配列 `a` は借りたまま、添字 `b` の要素の参照を `dst` に置く。
    ArrayGet {
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// `Std.Array.length` と `Std.MutArray.length`。配列 `a` は借りたまま。
    ArrayLength {
        dst: Slot,
        a: Slot,
    },
    /// `Std.Array.set` と `Std.MutArray.write`。`operands[at..]` は [配列, 添字, 値]。配列と値を消費する。
    ArraySet {
        dst: Slot,
        at: u32,
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
