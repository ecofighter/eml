use std::fmt;

use eml_hir::DisplayNames;

use crate::store::{TypeId, TypeKind, TypeStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// row に含まれてよい操作の上限。`Never` はマルチコア対応のために最初から持つ (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Multiplicity {
    Never,
    Once,
    Multi,
}

/// Kind の制約の片側。`Unr` と `Lin` は定数で、`Of` はその型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindTerm {
    Unr,
    Lin,
    Of(TypeId),
}

/// スキームに残った Kind の制約。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindConstraint {
    /// `lower <= upper`。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が `Lin` なら `row` は `Once` 以下である (docs/spec/types.md の「推論」)。
    Carry { value: KindTerm, row: RowTerm },
}

/// 持ち越しの制約の row の側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowTerm {
    Multi,
    /// rigid な row 変数の名前。
    Of(String),
}

impl fmt::Display for RowTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowTerm::Multi => f.write_str("Multi"),
            RowTerm::Of(name) => write!(f, "<{name}>"),
        }
    }
}

impl KindTerm {
    fn show(&self, types: &TypeStore, names: &DisplayNames) -> String {
        match *self {
            KindTerm::Unr => "Unr".to_string(),
            KindTerm::Lin => "Lin".to_string(),
            KindTerm::Of(ty) if matches!(types.kind(ty), TypeKind::Fn { .. }) => {
                format!("({})", types.display(ty, names))
            }
            KindTerm::Of(ty) => types.display(ty, names).to_string(),
        }
    }
}

impl KindConstraint {
    /// テストの表示。型の名前は表示名の表から引く。
    pub(crate) fn show(&self, types: &TypeStore, names: &DisplayNames) -> String {
        match self {
            KindConstraint::Linearity { lower, upper } => {
                format!(
                    "{} <= {}",
                    lower.show(types, names),
                    upper.show(types, names)
                )
            }
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => format!("{row} <= Once"),
            KindConstraint::Carry { value, row } => {
                format!("{} => {row} <= Once", value.show(types, names))
            }
        }
    }
}
