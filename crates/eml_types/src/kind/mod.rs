//! Kind の変数、`下限 ≤ 上限` の制約の両辺 (`Bound`)、束の値 (`Level`)、制約の由来、持ち越しの制約を置く
//! (docs/spec/types.md の「推論」)。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。
//! 段1が集める問題とスキームは `problem` に、段2が SCC ごとに解く処理は `solve` にある。

use eml_diagnostics::{FileId, TextRange, TextSize};
use eml_hir::OperationId;

use crate::table::Row;
use crate::ty::{Linearity, Multiplicity};

pub(crate) mod problem;
pub(crate) mod solve;

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
}

impl Level for Linearity {
    const BOTTOM: Self = Linearity::Unr;
}

impl Level for Multiplicity {
    const BOTTOM: Self = Multiplicity::Never;
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
/// `reason` の中の位置は、どれも由来を作った本体の中にあり、`span` と同じファイルである。ただし `CarriedThrough` の `inner` は自分の `Span` を持ち、呼ばれた関数のファイルを指しうる。
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
    /// 本体の検査の表の既定値で、由来を付け忘れた制約である。値は検査している関数の名前の位置。
    Unattributed(Span),
}

impl Provenance {
    pub fn origin(&self) -> Option<&KindOrigin> {
        match self {
            Provenance::At(origin) => Some(origin),
            Provenance::Suppressed | Provenance::Declaration | Provenance::Unattributed(_) => None,
        }
    }
}

/// `drop x` の行を入れる位置と、その行の字下げ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DropFix {
    pub offset: TextSize,
    pub indent: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindReason {
    /// ある経路で2回以上使った変数。名前と、その経路で1回目と2回目に使った位置。
    UsedMoreThanOnce {
        name: String,
        first: TextRange,
        second: TextRange,
    },
    /// ある経路で使わなかった変数。`fix` は `drop x` の行を入れる先である。
    NotUsed {
        name: String,
        path: UnusedPath,
        fix: Option<DropFix>,
    },
    /// `once` の操作の節の `k` を、ある経路で呼びも `drop` もしなかった。`clause` は節の範囲。
    ContinuationNotUsed { name: String, clause: TextRange },
    /// `_` で受けた値。
    Discarded,
    /// 操作の節が捕まえた変数。
    CapturedByClause(String),
    /// ラムダが捕まえた値。
    CapturedByLambda,
    /// トップレベルの関数、組み込み、操作のスキームから複写した制約。名前は参照した値の名前である。
    Passed(String),
    /// 型の単一化で出た制約。
    Unified,
    /// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を型の表から切り離し、
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
        /// 状態の型を表示した文字列。ラベルに出す。
        ty: String,
    },
}

impl KindReason {
    /// 同じ範囲の由来を並べる順。種類は宣言の順で、同じ種類は中身の名前と位置を順に比べる。中身の違う由来は鍵も違うので、
    /// 同じ値の持ち越しの違反から報告する1件を、制約が並んだ順に左右されずに選べる
    /// (docs/implementation/diagnostics.md の E3006)。
    pub fn order_key(&self) -> (u8, Vec<KeyPart>) {
        match self {
            KindReason::UsedMoreThanOnce {
                name,
                first,
                second,
            } => (0, [vec![text(name)], span(*first), span(*second)].concat()),
            KindReason::NotUsed { name, path, fix } => {
                let fix = match fix {
                    None => vec![number(0)],
                    Some(fix) => vec![number(1), number(fix.offset.into()), number(fix.indent)],
                };
                (1, [vec![text(name)], path.order_key(), fix].concat())
            }
            KindReason::ContinuationNotUsed { name, clause } => {
                (2, [vec![text(name)], span(*clause)].concat())
            }
            KindReason::Discarded => (3, Vec::new()),
            KindReason::CapturedByClause(name) => (4, vec![text(name)]),
            KindReason::CapturedByLambda => (5, Vec::new()),
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
            KindReason::OmittedReturn { ty } => (10, vec![text(ty)]),
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
    /// handler の状態。本体の実行中は handler フレームにある (docs/spec/effects.md の「パラメータ付き handler」)。
    HandlerState {
        /// `from` の初期値の式の範囲。
        init: TextRange,
    },
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
        };
        [span(self.key()), rest].concat()
    }
}

/// 値がまたぐもの。持ち越しのパスが多重度の成分を作るのに使う。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone)]
pub(crate) enum Across {
    Row(Row),
    Operation(OperationId),
}

/// 値がまたぐ式の種類。E3006 の primary の言い方を決める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallKind {
    Call,
    Handle,
}

impl CallKind {
    fn order_key(&self) -> Vec<KeyPart> {
        match self {
            CallKind::Call => vec![number(0)],
            CallKind::Handle => vec![number(1)],
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
                value: CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. },
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
    use eml_hir::ModuleId;
    use la_arena::{Idx, RawIdx};

    use super::*;

    #[test]
    fn reasons_have_a_fixed_order() {
        let used = KindReason::UsedMoreThanOnce {
            name: "f".to_string(),
            first: TextRange::new(1.into(), 2.into()),
            second: TextRange::new(3.into(), 4.into()),
        };
        let unified = KindReason::Unified;
        assert!(used.order_key() < unified.order_key());
        let a = KindReason::Passed("a".to_string());
        let b = KindReason::Passed("b".to_string());
        assert!(a.order_key() < b.order_key());

        let reasons = distinct_reasons();
        for (i, x) in reasons.iter().enumerate() {
            for y in &reasons[i + 1..] {
                assert_ne!(x.order_key(), y.order_key(), "{x:?} and {y:?}");
            }
        }
        let sorted = |mut list: Vec<KindReason>| {
            list.sort_by_cached_key(KindReason::order_key);
            list
        };
        let reversed = reasons.iter().rev().cloned().collect();
        let mut rotated = reasons.clone();
        rotated.rotate_left(reasons.len() / 2);
        assert_eq!(sorted(reversed), sorted(rotated));
    }

    /// 中身が1か所だけ違う由来を、種類ごとに並べる。
    fn distinct_reasons() -> Vec<KindReason> {
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
            not_used(
                UnusedPath::ScopeEnd(range(1, 1)),
                Some(DropFix {
                    offset: 1.into(),
                    indent: 2,
                }),
            ),
            not_used(
                UnusedPath::ScopeEnd(range(1, 1)),
                Some(DropFix {
                    offset: 1.into(),
                    indent: 4,
                }),
            ),
            KindReason::ContinuationNotUsed {
                name: "k".to_string(),
                clause: range(1, 9),
            },
            KindReason::Discarded,
            KindReason::CapturedByClause("f".to_string()),
            KindReason::CapturedByLambda,
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
            through(None),
            through(Some(inner(a, 1, InnerLabel::Value))),
            through(Some(inner(b, 1, InnerLabel::Value))),
            through(Some(inner(a, 1, InnerLabel::Kept("x".to_string())))),
            through(Some(inner(a, 1, InnerLabel::Through("keep2".to_string())))),
            through(Some(inner(a, 5, InnerLabel::Value))),
            KindReason::OmittedReturn {
                ty: "File".to_string(),
            },
            KindReason::OmittedReturn {
                ty: "String".to_string(),
            },
        ]
    }
}
