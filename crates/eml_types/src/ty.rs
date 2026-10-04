use std::fmt;

use eml_hir::{EffectId, TypeDefId};

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

/// 外に出す型の row のラベル。名前を持つのは、`Module` を渡さずに表示するため。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectLabel {
    pub id: EffectId,
    pub name: String,
}

/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、解けずに残った変数は `Flexible` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Con {
        id: TypeDefId,
        name: String,
    },
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, Type)>),
    Fn {
        param: Box<Type>,
        linearity: Linearity,
        effects: Vec<EffectLabel>,
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: Box<Type>,
    },
    /// シグネチャの型変数。
    Rigid(String),
    /// 推論で解けなかった変数。`_` と表示する。
    Flexible,
    Error,
}

/// row の末尾。シグネチャの row 変数は名前を持つ。推論で解けなかった row 変数は `_` と表示する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTail {
    Rigid(String),
    Flexible,
    /// 未定義のエフェクトか解決できない row 変数の跡。どのエフェクトも受け入れる。
    Error,
}

impl Type {
    pub fn unit() -> Type {
        Type::Record(Vec::new())
    }

    pub fn contains_error(&self) -> bool {
        match self {
            Type::Error => true,
            Type::Record(fields) => fields.iter().any(|(_, ty)| ty.contains_error()),
            Type::Fn {
                param, tail, ret, ..
            } => {
                param.contains_error()
                    || ret.contains_error()
                    || matches!(tail, Some(RowTail::Error))
            }
            Type::Con { .. } | Type::Rigid(_) | Type::Flexible => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Con { name, .. } => f.write_str(name),
            Type::Record(fields) if fields.is_empty() => f.write_str("Unit"),
            Type::Record(fields) => {
                f.write_str("{ ")?;
                for (index, (label, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label} : {ty}")?;
                }
                f.write_str(" }")
            }
            Type::Fn {
                param,
                effects,
                tail,
                ret,
                ..
            } => {
                if matches!(**param, Type::Fn { .. }) {
                    write!(f, "({param}) -> ")?;
                } else {
                    write!(f, "{param} -> ")?;
                }
                let names: Vec<&str> = effects.iter().map(|e| e.name.as_str()).collect();
                let tail = match tail {
                    Some(RowTail::Rigid(name)) => Some(name.as_str()),
                    Some(RowTail::Flexible) => Some("_"),
                    Some(RowTail::Error) => Some("{error}"),
                    None => None,
                };
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                match tail {
                    Some(tail) if names.is_empty() => write!(f, "<{tail}> ")?,
                    Some(tail) => write!(f, "<{} | {tail}> ", names.join(", "))?,
                    None if names.is_empty() => {}
                    None => write!(f, "<{}> ", names.join(", "))?,
                }
                write!(f, "{ret}")
            }
            Type::Rigid(name) => f.write_str(name),
            Type::Flexible => f.write_str("_"),
            Type::Error => f.write_str("{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use eml_hir::{EffectDef, TypeDef};
    use la_arena::Arena;

    use super::*;

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut con = |name: &str| Type::Con {
            id: types.alloc(TypeDef {
                name: name.to_string(),
            }),
            name: name.to_string(),
        };
        let pure = Type::Fn {
            param: Box::new(con("Int")),
            linearity: Linearity::Unr,
            effects: vec![],
            tail: None,
            ret: Box::new(con("Bool")),
        };
        let io = EffectLabel {
            id: effects.alloc(EffectDef {
                name: "IO".to_string(),
            }),
            name: "IO".to_string(),
        };
        let io = Type::Fn {
            param: Box::new(pure.clone()),
            linearity: Linearity::Unr,
            effects: vec![io],
            tail: None,
            ret: Box::new(Type::unit()),
        };
        assert_eq!(pure.to_string(), "Int -> Bool");
        assert_eq!(io.to_string(), "(Int -> Bool) -> <IO> Unit");
    }
}

/// Kind の制約の片側。`Unr` と `Lin` は定数で、`Of` はその型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindTerm {
    Unr,
    Lin,
    Of(Type),
}

/// スキームに残った Kind の制約 `lower <= upper`。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindConstraint {
    pub lower: KindTerm,
    pub upper: KindTerm,
}

impl fmt::Display for KindTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KindTerm::Unr => f.write_str("Unr"),
            KindTerm::Lin => f.write_str("Lin"),
            KindTerm::Of(ty @ Type::Fn { .. }) => write!(f, "({ty})"),
            KindTerm::Of(ty) => write!(f, "{ty}"),
        }
    }
}

impl fmt::Display for KindConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} <= {}", self.lower, self.upper)
    }
}
