//! Kind の変数、`下限 ≤ 上限` の制約の両辺 (`Bound`)、束の値 (`Level`)、制約の由来、持ち越しの制約を置く
//! (docs/spec/types.md の「推論」)。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。
//! 段1が集める問題とスキームは `problem` に、段2が SCC ごとに解く処理は `solve` に、解いたスキームの上の評価は `eval` に、
//! 本体の型を書き出すときの変数の値は `summary` にある。

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::{DisplayNames, EffectId, OperationId, VarId};

use crate::store::{TypeId, TypeStore};
use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};

pub(crate) mod entail;
pub(crate) mod eval;
pub(crate) mod graph;
pub(crate) mod problem;
pub(crate) mod solve;
pub(crate) mod summary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KindVar(u32);

impl KindVar {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: usize) -> KindVar {
        KindVar(index as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Bound<T> {
    Const(T),
    Var(KindVar),
}

/// Kind の束の値。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の2つがある (docs/spec/types.md の「Kind」)。
pub(crate) trait Level: Copy + Ord + std::hash::Hash + std::fmt::Debug {
    const BOTTOM: Self;
    const TOP: Self;
}

impl Level for Linearity {
    const BOTTOM: Self = Linearity::Unr;
    const TOP: Self = Linearity::Lin;
}

impl Level for Multiplicity {
    const BOTTOM: Self = Multiplicity::Never;
    const TOP: Self = Multiplicity::Multi;
}

/// ファイルを持つ位置。由来は別のモジュールの中を指しうる
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub file: FileId,
    pub range: TextRange,
}

/// Kind の制約の由来。制約が破れたときに E3001〜E3005 が指す場所と理由である
/// (docs/implementation/diagnostics.md の「線形性の診断」)。
/// `reason` の中の位置は、どれも由来を作った本体の中にあり、`span` と同じファイルである。ただし、自分の `Span` を持つ
/// 次の位置は例外である。`CarriedThrough` の `inner` は、呼ばれた関数のファイルを指しうる。`RowBound` の `source` は、
/// 参照した宣言のシグネチャか本体、または同じ SCC の別のメンバーの本体を指し、別のファイルでもありうる。`RowBound` の
/// `cause` は `span` と同じファイルだが、同じ SCC の別のメンバーの本体を指しうる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KindOrigin {
    pub span: Span,
    pub reason: KindReason,
}

/// Kind の制約の由来の種類。由来を付け忘れた制約の違反を、黙って捨てずに見つけるため、由来を省けない形にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// 報告する由来。
    At(KindOrigin),
    /// 誤りの跡がある本体 (`usage::reliable` が偽) の、使用回数と持ち越しの制約。正しく数えられないので、違反しても
    /// 報告しない (docs/spec/diagnostics.md)。
    Suppressed,
    /// 宣言の型と、SCC の中の参照の等式から作る制約。それだけでは破れない。具体化するときは参照した位置の由来を付けて
    /// 複写する。
    Declaration,
    /// 本体の検査で、制約 `C a` と instance の頭の型変数が意味する `a ≤ Unr`
    /// (docs/spec/types.md の「`Unr` のクラス」)。宣言から出るが、SCC の中の参照が
    /// `a` を線形な型で具体化すると破れる。そのとき、参照が求める制約は解けないか (E2006)、線形な頭の instance で解けるか
    /// (E2010)、型引数に `Unr` を求めて破れるので、誤りは報告済みである。そこで報告しない。
    Given,
    /// 本体の検査の表の既定値で、由来を付け忘れた制約である。値は検査している関数の名前の位置。
    Unattributed(Span),
    /// シグネチャに書いた row 変数の上限。値はその `never` の位置である。破れるのは同じ SCC の参照を通したときだけで、
    /// その参照に複写した制約が報告する (docs/spec/types.md の「row 変数の多重度の上限」)。
    Bound(Span),
}

impl Provenance {
    pub fn origin(&self) -> Option<&KindOrigin> {
        match self {
            Provenance::At(origin) => Some(origin),
            Provenance::Suppressed
            | Provenance::Declaration
            | Provenance::Given
            | Provenance::Unattributed(_)
            | Provenance::Bound(_) => None,
        }
    }
}

/// 操作の型が節に求める制約の出どころ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationSide {
    /// 引数の型。操作のスキームが `Unr` に固定している。
    Argument,
    /// 結果の型。節が作って渡す関数は、呼ぶ側が何回でも呼べる。
    Result,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindReason {
    /// ある経路で2回以上使った変数。名前と、その経路で1回目と2回目に使った位置。
    UsedMoreThanOnce {
        name: String,
        first: TextRange,
        second: TextRange,
    },
    /// ある経路で使わなかった変数。`fix` は `drop x` の行を入れる位置である。行の字下げは、報告するときにソースから
    /// 求める。
    NotUsed {
        name: String,
        path: UnusedPath,
        fix: Option<TextSize>,
    },
    /// `once` の操作の節の `k` を、ある経路で呼びも `drop` もしなかった。`clause` は節の範囲。
    ContinuationNotUsed { name: String, clause: TextRange },
    /// `_` で受けた値。
    Discarded,
    /// 操作の節が捕まえた変数。
    CapturedByClause(String),
    /// ラムダが捕まえた値。
    CapturedByLambda,
    /// 再帰する組のローカル関数の型。`name` の関数の本体が、組の関数 `callee` (自分を含む) を参照する。由来の範囲は
    /// その参照の位置である。制約を受けた関数が組の関数を参照しないときは、由来は組の中の最初の参照になり、`name` は
    /// その関数を指さない。そのため `member` に、制約を受けた関数の名前と束縛の位置を持つ
    /// (docs/implementation/diagnostics.md の「線形性の診断」)。
    RecursiveLocal {
        name: String,
        callee: String,
        member: Option<(String, TextRange)>,
    },
    /// トップレベルの関数、組み込み、操作のスキームから複写した制約。名前は参照した値の名前である。
    Passed(String),
    /// 操作の型が、その操作の節に求める制約。名前は操作の名前で、由来の範囲は節の頭 (操作の名前から最後の引数まで)
    /// である (docs/spec/effects.md の「handler の意味」)。`lambda` は、違反したときに線形の下限を作ったラムダの
    /// 範囲で、段2が解いた後に入れる。
    OperationType {
        name: String,
        side: OperationSide,
        lambda: Option<TextRange>,
    },
    /// 型の単一化で出た制約。
    Unified,
    /// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を推論の表から切り離し、
    /// スキームに残せるようにするため。
    CarriedAcross {
        value: CarriedValue,
        multi: Option<OperationId>,
        call: CallKind,
    },
    /// スキームから複写した持ち越しの制約。由来の範囲は参照した位置である。`inner` は、呼んだ関数の中で値をまたがせている
    /// 位置の要約である (docs/implementation/diagnostics.md の E3006)。
    CarriedThrough {
        name: String,
        inner: Option<CarriedInner>,
    },
    /// 状態のある handler で、省いた `return` の節が状態を `_` で捨てた (docs/spec/expressions.md の「パラメータ付き
    /// handler」)。由来の範囲は `from` の初期値の式である。
    OmittedReturn {
        /// 状態の型。ラベルに出す。表示するのは破れた制約を報告するときだけである。
        ty: TypeId,
    },
    /// レコードのパターンに書かなかったフィールドと、射影と更新が捨てるフィールド。由来の範囲は、パターン全体か、射影か
    /// 更新の式である。書き手の `_` と区別して、フィールドの名前で報告する
    /// (docs/implementation/diagnostics.md の「線形性の診断」)。
    DiscardedField { name: String, site: DiscardSite },
    /// `Unr` の変数に分けた `var` の変数の型。由来の範囲は `var` の文である。`ty` は変数の型で、破れたときの文言に出す
    /// (docs/spec/types.md の「`var` の変数」)。
    VarValue {
        var: VarId,
        name: String,
        ty: TypeId,
    },
    /// 配列の変数を、ある経路で2回以上取り出した。由来の範囲は `var` の文で、`first` と `second` はその経路の1回目と
    /// 2回目の取り出しである (docs/spec/linearity.md の「配列の変数」)。
    VarTakenTwice {
        name: String,
        first: TextRange,
        second: TextRange,
    },
    /// 配列の変数を、ある経路で取り出さなかった。由来の範囲は `var` の文である。
    VarNotTaken { name: String, path: UntakenPath },
    /// 配列の変数の、取り出しの規則に合わない使い方 (E3008)。由来の範囲はその式で、`decl` は `var` の文である。
    ArrayVarMisused {
        name: String,
        decl: TextRange,
        misuse: VarMisuse,
    },
    /// 参照した宣言 `name` の row 変数の上限 (E2020)。由来の範囲は参照の位置である。`source` は上限の出どころで、
    /// `cause` は上限を超えた row がどこから来たかを表す。`cause` は段2が解いた後に入れる
    /// (docs/implementation/diagnostics.md の E2020)。
    RowBound {
        name: String,
        source: BoundSource,
        cause: Option<BoundCause>,
    },
    /// row 変数に入れたラベルのエフェクト。由来の範囲はその row を入れた式である。下限の制約にだけ付くので破れず、
    /// E2020 の原因として読まれる。
    Performs(EffectId),
}

/// 宣言のスキームに残した row 変数の上限の出どころ。E2020 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoundSource {
    /// シグネチャに書いた `never`。
    Written(Span),
    /// 推論した上限で、`name` に row を渡した参照から来た。その参照は、上限を残した宣言 (参照された側) の本体か、同じ
    /// SCC の別のメンバーの本体にある。報告する参照を含む本体にはない。
    Through { name: String, span: Span },
    /// 出どころを決めなかった。
    Unknown,
}

/// 上限を超えた row が来た所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoundCause {
    pub span: Span,
    pub what: Exceeding,
}

/// 上限を超えたもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exceeding {
    /// そこで起こすラベルのエフェクト。
    Effect(EffectId),
    /// エフェクトの分からない row の多重度。
    Multiplicity(Multiplicity),
}

impl BoundSource {
    fn order_key(&self) -> Vec<KeyPart> {
        match self {
            BoundSource::Written(written) => [
                vec![number(0), KeyPart::File(written.file)],
                span(written.range),
            ]
            .concat(),
            BoundSource::Through { name, span: at } => [
                vec![number(1), KeyPart::File(at.file)],
                span(at.range),
                vec![text(name)],
            ]
            .concat(),
            BoundSource::Unknown => vec![number(2)],
        }
    }
}

impl BoundCause {
    fn order_key(&self) -> Vec<KeyPart> {
        let what = match self.what {
            Exceeding::Effect(effect) => vec![
                number(0),
                number(effect.module.into_raw().into_u32()),
                number(effect.local.into_raw().into_u32()),
            ],
            Exceeding::Multiplicity(m) => vec![number(1), number(m as u32)],
        };
        [
            vec![KeyPart::File(self.span.file)],
            span(self.span.range),
            what,
        ]
        .concat()
    }
}

/// 配列の変数を取り出さなかった経路。E3003 の secondary が指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UntakenPath {
    /// `if` の枝、または `match` の枝の本体。
    Branch(TextRange),
    /// `else` のない `if`。
    NoElse(TextRange),
    /// どの経路でも取り出さなかった。範囲は最後の式である。
    Last(TextRange),
    /// ブロックが文で終わり、最後の式がない。範囲はブロックの終わりの長さ0の範囲である。
    NoLast(TextRange),
}

impl UntakenPath {
    fn order_key(self) -> Vec<KeyPart> {
        let (kind, range) = match self {
            UntakenPath::Branch(range) => (0, range),
            UntakenPath::NoElse(range) => (1, range),
            UntakenPath::Last(range) => (2, range),
            UntakenPath::NoLast(range) => (3, range),
        };
        [vec![number(kind)], span(range)].concat()
    }
}

/// E3008 に当たる配列の変数の使い方 (docs/spec/linearity.md の「配列の変数」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarMisuse {
    /// 変数のブロックの最後の式の外で取り出した。
    TakenOutsideLast,
    /// 最後の式の中の関数 (ラムダ、局所関数、handle の本体と節、`use` の残り、セクション) で取り出した。
    TakenInFunction,
    /// 最後の式の中で、取り出しのほかに変数を使った。
    UsedBesideTake,
    /// `x <- e` で代入した。
    Assigned,
}

/// フィールドを捨てた式の種類。E3004 の言い方と help を変える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscardSite {
    /// レコードのパターンに書かなかった。
    Pattern,
    /// 射影が取り出さなかった残り。タプルの要素なら `tuple` が真である。セクションから組んだラムダの中では、隠れた
    /// 引数の分解を書けないので help を付けない (`section` が真)。
    Projection { tuple: bool, section: bool },
    /// 更新が上書きした古い値。
    Update,
}

impl DiscardSite {
    fn order_key(self) -> u32 {
        match self {
            DiscardSite::Pattern => 0,
            DiscardSite::Projection { tuple, section } => {
                1 + u32::from(tuple) * 2 + u32::from(section)
            }
            DiscardSite::Update => 5,
        }
    }
}

impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は中身の名前と位置を順に比べる。中身の違う由来は鍵も違うので、
    /// 同じ値の持ち越しの違反から報告する1件を、制約が並んだ順に左右されずに選べる
    /// (docs/implementation/diagnostics.md の E3006)。型は表示した文字列で比べる。ID は検査の順で決まるので、鍵に
    /// すると並びが検査の順に左右される。
    pub fn order_key(&self, types: &TypeStore, names: &DisplayNames) -> (u8, Vec<KeyPart>) {
        match self {
            KindReason::UsedMoreThanOnce {
                name,
                first,
                second,
            } => (0, [vec![text(name)], span(*first), span(*second)].concat()),
            KindReason::NotUsed { name, path, fix } => {
                let fix = match *fix {
                    None => vec![number(0)],
                    Some(offset) => vec![number(1), number(offset.into())],
                };
                (1, [vec![text(name)], path.order_key(), fix].concat())
            }
            KindReason::ContinuationNotUsed { name, clause } => {
                (2, [vec![text(name)], span(*clause)].concat())
            }
            KindReason::Discarded => (3, Vec::new()),
            KindReason::CapturedByClause(name) => (4, vec![text(name)]),
            KindReason::CapturedByLambda => (5, Vec::new()),
            KindReason::RecursiveLocal {
                name,
                callee,
                member,
            } => {
                let member = match member {
                    None => vec![number(0)],
                    Some((member, range)) => [vec![number(1), text(member)], span(*range)].concat(),
                };
                (5, [vec![text(name), text(callee)], member].concat())
            }
            KindReason::Passed(name) => (6, vec![text(name)]),
            KindReason::Unified => (7, Vec::new()),
            KindReason::CarriedAcross { value, multi, call } => {
                let multi = match multi {
                    None => vec![number(0)],
                    Some(op) => vec![
                        number(1),
                        number(op.module.into_raw().into_u32()),
                        number(op.local.into_raw().into_u32()),
                    ],
                };
                (8, [value.order_key(), multi, call.order_key()].concat())
            }
            KindReason::CarriedThrough { name, inner } => {
                let inner = match inner {
                    None => vec![number(0)],
                    Some(inner) => [
                        vec![number(1), KeyPart::File(inner.span.file)],
                        span(inner.span.range),
                        inner.label.order_key(),
                    ]
                    .concat(),
                };
                (9, [vec![text(name)], inner].concat())
            }
            KindReason::OmittedReturn { ty } => (
                10,
                vec![KeyPart::Text(types.display(*ty, names).to_string())],
            ),
            KindReason::DiscardedField { name, site } => {
                (11, vec![text(name), number(site.order_key())])
            }
            KindReason::VarValue { var, name, .. } => {
                (13, vec![text(name), number(var.into_raw().into_u32())])
            }
            KindReason::VarTakenTwice {
                name,
                first,
                second,
            } => (14, [vec![text(name)], span(*first), span(*second)].concat()),
            KindReason::VarNotTaken { name, path } => {
                (15, [vec![text(name)], path.order_key()].concat())
            }
            KindReason::ArrayVarMisused { name, decl, misuse } => (
                16,
                [vec![text(name)], span(*decl), vec![number(*misuse as u32)]].concat(),
            ),
            KindReason::RowBound {
                name,
                source,
                cause,
            } => {
                let cause = match cause {
                    None => vec![number(0)],
                    Some(cause) => [vec![number(1)], cause.order_key()].concat(),
                };
                (17, [vec![text(name)], source.order_key(), cause].concat())
            }
            KindReason::Performs(effect) => (
                18,
                vec![
                    number(effect.module.into_raw().into_u32()),
                    number(effect.local.into_raw().into_u32()),
                ],
            ),
            KindReason::OperationType { name, side, lambda } => {
                let lambda = match lambda {
                    None => vec![number(0)],
                    Some(range) => [vec![number(1)], span(*range)].concat(),
                };
                (
                    12,
                    [vec![text(name), number(*side as u32)], lambda].concat(),
                )
            }
        }
    }
}

/// `KindReason::order_key` の鍵の1要素。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum KeyPart {
    Number(u32),
    Text(String),
    File(FileId),
}

fn number(value: u32) -> KeyPart {
    KeyPart::Number(value)
}

fn text(name: &str) -> KeyPart {
    KeyPart::Text(name.to_string())
}

fn span(range: TextRange) -> Vec<KeyPart> {
    vec![number(range.start().into()), number(range.end().into())]
}

/// 変数を使わなかった経路。E3003 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnusedPath {
    /// `if` の枝、または `match` の枝の本体。
    Branch(TextRange),
    /// `else` のない `if`。
    NoElse(TextRange),
    /// どの経路でも使わなかった。範囲はスコープの終わりの長さ0の範囲である。
    ScopeEnd(TextRange),
    /// どの経路でも使わないうちに、同じブロックの後の `let` で隠された。範囲は隠した束縛である。
    Shadowed(TextRange),
}

impl UnusedPath {
    fn order_key(&self) -> Vec<KeyPart> {
        let (kind, range) = match self {
            UnusedPath::Branch(range) => (0, range),
            UnusedPath::NoElse(range) => (1, range),
            UnusedPath::ScopeEnd(range) => (2, range),
            UnusedPath::Shadowed(range) => (3, range),
        };
        [vec![number(kind)], span(*range)].concat()
    }
}

/// 呼び出しをまたいで持っている値。E3006 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CarriedValue {
    /// 局所変数。名前と束縛の範囲。
    Local { name: String, binding: TextRange },
    /// 評価済みで消費前の部分式の値。部分式の範囲。
    Temporary(TextRange),
    /// `return` の節が捕まえた変数。名前、束縛の範囲、節の範囲。
    ReturnCapture {
        name: String,
        binding: TextRange,
        clause: TextRange,
    },
    /// handler の状態。本体の実行中は handler の実行の evidence にある (docs/spec/effects.md の「パラメータ付き handler」)。
    HandlerState {
        /// `from` の初期値の式の範囲。
        init: TextRange,
    },
    /// `fn` の節の隠れた継続 `$k`。書き手は名前を知らないので、節の範囲で指す (docs/spec/effects.md の
    /// 「継続の多重度と持ち越し規則」)。
    FnClauseContinuation { clause: TextRange },
    /// 配列の変数の状態。範囲の実行中は変数の handler の evidence にある (docs/spec/linearity.md の「配列の変数」)。
    /// `decl` は `var` の文の範囲である。
    VarState { name: String, decl: TextRange },
}

impl CarriedValue {
    /// 同じ値を見分ける範囲。同じ値の違反は1件だけ報告する (docs/implementation/diagnostics.md の E3006)。
    pub fn key(&self) -> TextRange {
        match self {
            CarriedValue::Local { binding, .. } | CarriedValue::ReturnCapture { binding, .. } => {
                *binding
            }
            CarriedValue::Temporary(range) => *range,
            CarriedValue::HandlerState { init } => *init,
            CarriedValue::FnClauseContinuation { clause } => *clause,
            CarriedValue::VarState { decl, .. } => *decl,
        }
    }

    /// 同じ値を見分ける範囲を先に比べ、同じ値の由来を隣に並べる。
    fn order_key(&self) -> Vec<KeyPart> {
        let rest = match self {
            CarriedValue::Local { name, .. } => vec![number(0), text(name)],
            CarriedValue::Temporary(_) => vec![number(1)],
            CarriedValue::ReturnCapture { name, clause, .. } => {
                [vec![number(2), text(name)], span(*clause)].concat()
            }
            CarriedValue::HandlerState { .. } => vec![number(3)],
            CarriedValue::FnClauseContinuation { .. } => vec![number(4)],
            CarriedValue::VarState { name, .. } => vec![number(5), text(name)],
        };
        [span(self.key()), rest].concat()
    }
}

/// 値がまたぐもの。持ち越しのパスが多重度の成分を作るのに使う。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Across<'a> {
    Row(&'a Row),
    Operation(OperationId),
}

/// 値がまたぐものの多重度の成分と、報告が指す `multi` の操作。持ち越しのパスが、またぐ式ごとに1回だけ求める。
/// 成分が空なら制約を出さない。
pub(crate) struct Crossing {
    pub mults: Vec<Bound<Multiplicity>>,
    pub multi: Option<OperationId>,
}

/// 値がまたぐ式の種類。E3006 の primary の言い方を決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallKind {
    Call,
    Handle,
    /// `var` の範囲。変数の handle と同じく、範囲の実行が外側の row をまたぐ。
    Var,
}

impl CallKind {
    fn order_key(&self) -> Vec<KeyPart> {
        match self {
            CallKind::Call => vec![number(0)],
            CallKind::Handle => vec![number(1)],
            CallKind::Var => vec![number(2)],
        }
    }
}

/// `CarriedThrough` が指す、呼んだ関数の中の持ち越しの1段分。報告は1段しかたどらないので入れ子にしない。入れ子にすると、
/// 呼び出しの段数だけ由来が深くなり、複写のたびにその深さの時間がかかる (docs/implementation/diagnostics.md の E3006)。
/// 呼ばれた関数の中を指すので、呼んだ側の由来とは別のファイルでありうる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CarriedInner {
    pub span: Span,
    pub label: InnerLabel,
}

/// 報告の secondary の言い方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InnerLabel {
    /// 名前のある値を持ったまま呼んだ。
    Kept(String),
    /// さらに別の関数を通った。
    Through(String),
    /// 名前のない値を持ったまま呼んだ。
    Value,
}

impl InnerLabel {
    fn order_key(&self) -> Vec<KeyPart> {
        match self {
            InnerLabel::Kept(name) => vec![number(0), text(name)],
            InnerLabel::Through(name) => vec![number(1), text(name)],
            InnerLabel::Value => vec![number(2)],
        }
    }
}

impl CarriedInner {
    pub fn of(origin: &KindOrigin) -> CarriedInner {
        let label = match &origin.reason {
            KindReason::CarriedAcross {
                value:
                    CarriedValue::Local { name, .. }
                    | CarriedValue::ReturnCapture { name, .. }
                    | CarriedValue::VarState { name, .. },
                ..
            } => InnerLabel::Kept(name.clone()),
            KindReason::CarriedThrough { name, .. } => InnerLabel::Through(name.clone()),
            _ => InnerLabel::Value,
        };
        CarriedInner {
            span: origin.span,
            label,
        }
    }
}

/// 持ち越しの制約 (docs/spec/types.md の「推論」)。`lin` の解が `Lin` なら、`mult` の解は `Once` 以下でなければならない。
/// 線形性と多重度の束とは別に持つのは、スキームに残すときも由来を持つためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Carry {
    pub lin: Bound<Linearity>,
    pub mult: Bound<Multiplicity>,
    pub origin: Provenance,
}

#[cfg(test)]
mod tests {
    use eml_extern::ExternType;
    use eml_hir::{ModuleId, Program};
    use la_arena::{Idx, RawIdx};

    use super::*;
    use crate::store::TypeKind;

    #[test]
    fn reasons_have_a_fixed_order() {
        let program = crate::test_program("import Std.FileIn");
        let mut types = TypeStore::for_program(&program);
        let reasons = distinct_reasons(&program, &mut types);
        let key = |reason: &KindReason| reason.order_key(&types, &program.names);
        let used = KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: TextRange::new(1.into(), 2.into()),
            second: TextRange::new(3.into(), 4.into()),
        };
        let unified = KindReason::Unified;
        assert!(key(&used) < key(&unified));
        let a = KindReason::Passed("a".to_string());
        let b = KindReason::Passed("b".to_string());
        assert!(key(&a) < key(&b));

        for (i, x) in reasons.iter().enumerate() {
            for y in &reasons[i + 1..] {
                assert_ne!(key(x), key(y), "{x:?} and {y:?}");
            }
        }
        let sorted = |mut list: Vec<KindReason>| {
            list.sort_by_cached_key(key);
            list
        };
        let reversed = reasons.iter().rev().cloned().collect();
        let mut rotated = reasons.clone();
        rotated.rotate_left(reasons.len() / 2);
        assert_eq!(sorted(reversed), sorted(rotated));
    }

    /// 中身が1か所だけ違う由来を、種類ごとに並べる。
    fn distinct_reasons(program: &Program, types: &mut TypeStore) -> Vec<KindReason> {
        let file = types.intern(TypeKind::Con {
            id: program.extern_type(ExternType::FileIn),
            args: Vec::new(),
            rows: Vec::new(),
        });
        let range = |start: u32, end: u32| TextRange::new(start.into(), end.into());
        let op = |index: u32| {
            OperationId::new(
                ModuleId::from_raw(RawIdx::from(0)),
                Idx::from_raw(RawIdx::from(index)),
            )
        };
        let local = |name: &str, start| CarriedValue::Local {
            name: name.to_string(),
            binding: range(start, start + 1),
        };
        let used = |second| KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: range(1, 2),
            second,
        };
        let not_used = |path, fix| KindReason::NotUsed {
            name: "f".to_string(),
            path,
            fix,
        };
        let across = |value, multi, call| KindReason::CarriedAcross { value, multi, call };
        let through = |inner| KindReason::CarriedThrough {
            name: "keep".to_string(),
            inner,
        };
        let mut files = eml_diagnostics::SourceFiles::new();
        let a = files.add("a.em", "");
        let b = files.add("b.em", "");
        let at = |file, start| Span {
            file,
            range: range(start, start + 1),
        };
        let effect = |index: u32| {
            EffectId::new(
                ModuleId::from_raw(RawIdx::from(0)),
                Idx::from_raw(RawIdx::from(index)),
            )
        };
        let row_bound = |name: &str, source, cause| KindReason::RowBound {
            name: name.to_string(),
            source,
            cause,
        };
        let inner = |file, start, label| CarriedInner {
            span: Span {
                file,
                range: range(start, start + 1),
            },
            label,
        };
        vec![
            used(range(3, 4)),
            used(range(5, 6)),
            not_used(UnusedPath::Branch(range(1, 2)), None),
            not_used(UnusedPath::NoElse(range(1, 2)), None),
            not_used(UnusedPath::ScopeEnd(range(1, 1)), None),
            not_used(UnusedPath::ScopeEnd(range(1, 1)), Some(1.into())),
            KindReason::ContinuationNotUsed {
                name: "k".to_string(),
                clause: range(1, 9),
            },
            KindReason::Discarded,
            KindReason::CapturedByClause("f".to_string()),
            KindReason::CapturedByLambda,
            KindReason::RecursiveLocal {
                name: "go".to_string(),
                callee: "go".to_string(),
                member: None,
            },
            KindReason::RecursiveLocal {
                name: "go".to_string(),
                callee: "loop".to_string(),
                member: None,
            },
            KindReason::RecursiveLocal {
                name: "loop".to_string(),
                callee: "go".to_string(),
                member: None,
            },
            KindReason::RecursiveLocal {
                name: "loop".to_string(),
                callee: "go".to_string(),
                member: Some(("close".to_string(), range(1, 2))),
            },
            KindReason::Unified,
            across(local("f", 1), None, CallKind::Call),
            across(local("f", 1), Some(op(0)), CallKind::Call),
            across(local("f", 1), Some(op(1)), CallKind::Call),
            across(local("f", 1), Some(op(0)), CallKind::Handle),
            across(local("g", 1), Some(op(0)), CallKind::Call),
            across(local("f", 3), Some(op(0)), CallKind::Call),
            across(
                CarriedValue::Temporary(range(1, 2)),
                Some(op(0)),
                CallKind::Call,
            ),
            across(
                CarriedValue::ReturnCapture {
                    name: "f".to_string(),
                    binding: range(1, 2),
                    clause: range(5, 9),
                },
                Some(op(0)),
                CallKind::Handle,
            ),
            across(
                CarriedValue::HandlerState { init: range(1, 2) },
                Some(op(0)),
                CallKind::Handle,
            ),
            across(
                CarriedValue::FnClauseContinuation {
                    clause: range(1, 2),
                },
                Some(op(0)),
                CallKind::Call,
            ),
            through(None),
            through(Some(inner(a, 1, InnerLabel::Value))),
            through(Some(inner(b, 1, InnerLabel::Value))),
            through(Some(inner(a, 1, InnerLabel::Kept("x".to_string())))),
            through(Some(inner(a, 1, InnerLabel::Through("keep2".to_string())))),
            through(Some(inner(a, 5, InnerLabel::Value))),
            KindReason::OmittedReturn { ty: file },
            KindReason::OmittedReturn { ty: types.string() },
            KindReason::DiscardedField {
                name: "log".to_string(),
                site: DiscardSite::Pattern,
            },
            KindReason::DiscardedField {
                name: "log".to_string(),
                site: DiscardSite::Update,
            },
            KindReason::DiscardedField {
                name: "name".to_string(),
                site: DiscardSite::Pattern,
            },
            across(
                CarriedValue::VarState {
                    name: "a".to_string(),
                    decl: range(1, 2),
                },
                Some(op(0)),
                CallKind::Var,
            ),
            across(local("f", 1), Some(op(0)), CallKind::Var),
            KindReason::VarTakenTwice {
                name: "a".to_string(),
                first: range(1, 2),
                second: range(3, 4),
            },
            KindReason::VarNotTaken {
                name: "a".to_string(),
                path: UntakenPath::Branch(range(1, 2)),
            },
            KindReason::VarNotTaken {
                name: "a".to_string(),
                path: UntakenPath::NoElse(range(1, 2)),
            },
            KindReason::VarNotTaken {
                name: "a".to_string(),
                path: UntakenPath::Last(range(1, 2)),
            },
            KindReason::VarNotTaken {
                name: "a".to_string(),
                path: UntakenPath::NoLast(range(1, 1)),
            },
            KindReason::ArrayVarMisused {
                name: "a".to_string(),
                decl: range(1, 2),
                misuse: VarMisuse::TakenOutsideLast,
            },
            KindReason::ArrayVarMisused {
                name: "a".to_string(),
                decl: range(1, 2),
                misuse: VarMisuse::Assigned,
            },
            row_bound("helper", BoundSource::Unknown, None),
            row_bound("other", BoundSource::Unknown, None),
            row_bound("helper", BoundSource::Written(at(a, 1)), None),
            row_bound("helper", BoundSource::Written(at(b, 1)), None),
            row_bound(
                "helper",
                BoundSource::Through {
                    name: "wrap".to_string(),
                    span: at(a, 1),
                },
                None,
            ),
            row_bound(
                "helper",
                BoundSource::Unknown,
                Some(BoundCause {
                    span: at(a, 3),
                    what: Exceeding::Effect(effect(0)),
                }),
            ),
            row_bound(
                "helper",
                BoundSource::Unknown,
                Some(BoundCause {
                    span: at(a, 3),
                    what: Exceeding::Effect(effect(1)),
                }),
            ),
            row_bound(
                "helper",
                BoundSource::Unknown,
                Some(BoundCause {
                    span: at(a, 3),
                    what: Exceeding::Multiplicity(Multiplicity::Multi),
                }),
            ),
            KindReason::Performs(effect(0)),
            KindReason::Performs(effect(1)),
        ]
    }
}
