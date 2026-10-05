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
    pub args: Vec<Type>,
}

impl fmt::Display for EffectLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        for arg in &self.args {
            write!(f, " {}", atomic(arg))?;
        }
        Ok(())
    }
}

/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、解けずに残った変数は `Flexible` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// 型構成子とその型引数。`Int` などの組み込みの型は引数を持たない。
    Con {
        id: TypeDefId,
        name: String,
        args: Vec<Type>,
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
    /// 継続 `k` の型 (docs/spec/effects.md)。表面の構文に名前を持たず、診断の表示だけに使う。
    Cont {
        /// 操作の結果の型。`resume` に渡す値の型である。
        arg: Box<Type>,
        /// handle の結果の型。`resume` の値の型である。
        ret: Box<Type>,
        /// handle の外側の row。`resume` が起こすエフェクトである。
        effects: Vec<EffectLabel>,
        tail: Option<RowTail>,
        linearity: Linearity,
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
                param,
                effects,
                tail,
                ret,
                ..
            } => {
                param.contains_error()
                    || ret.contains_error()
                    || effects
                        .iter()
                        .any(|e| e.args.iter().any(Type::contains_error))
                    || matches!(tail, Some(RowTail::Error))
            }
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
                ..
            } => {
                arg.contains_error()
                    || ret.contains_error()
                    || effects
                        .iter()
                        .any(|e| e.args.iter().any(Type::contains_error))
                    || matches!(tail, Some(RowTail::Error))
            }
            Type::Con { args, .. } => args.iter().any(Type::contains_error),
            Type::Rigid(_) | Type::Flexible => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Con { name, args, .. } => {
                f.write_str(name)?;
                for arg in args {
                    write!(f, " {}", atomic(arg))?;
                }
                Ok(())
            }
            Type::Record(fields) if fields.is_empty() => f.write_str("Unit"),
            // ラベルが 0 から連番の閉じたレコードは、タプルの書き方で表示する (docs/spec/records.md の「構成」)
            Type::Record(fields) if is_tuple(fields) => {
                f.write_str("(")?;
                for (index, (_, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                f.write_str(")")
            }
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
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let row = row_text(effects, tail);
                if !row.is_empty() {
                    write!(f, "<{row}> ")?;
                }
                write!(f, "{ret}")
            }
            // 継続の row は、空でも書く。何も起こさない `resume` であることを示すため
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
                ..
            } => write!(
                f,
                "Cont {} {} <{}>",
                atomic(arg),
                atomic(ret),
                row_text(effects, tail)
            ),
            Type::Rigid(name) => f.write_str(name),
            Type::Flexible => f.write_str("_"),
            Type::Error => f.write_str("{error}"),
        }
    }
}

/// タプルとして書くレコード。タプルの構文は要素を2つ以上持つので、要素が1つのレコードは `{ 0 : A }` のまま書く。
fn is_tuple(fields: &[(String, Type)]) -> bool {
    fields.len() >= 2
        && fields
            .iter()
            .enumerate()
            .all(|(index, (label, _))| *label == index.to_string())
}

/// row の中身。`<` と `>` は呼び出し側が付ける。
fn row_text(effects: &[EffectLabel], tail: &Option<RowTail>) -> String {
    let names = effects
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<String>>();
    let tail = match tail {
        Some(RowTail::Rigid(name)) => Some(name.as_str()),
        Some(RowTail::Flexible) => Some("_"),
        Some(RowTail::Error) => Some("{error}"),
        None => None,
    };
    match tail {
        Some(tail) if names.is_empty() => tail.to_string(),
        Some(tail) => format!("{} | {tail}", names.join(", ")),
        None => names.join(", "),
    }
}

/// 型の適用の引数の位置に置く形。関数型、継続の型、引数を持つ型の適用は括弧で囲む。
fn atomic(ty: &Type) -> String {
    match ty {
        Type::Fn { .. } | Type::Cont { .. } => format!("({ty})"),
        Type::Con { args, .. } if !args.is_empty() => format!("({ty})"),
        _ => ty.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use eml_hir::{EffectDef, Generics, TypeDef};
    use la_arena::Arena;

    use super::*;

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut con = |name: &str| Type::Con {
            id: types.alloc(TypeDef::builtin(name)),
            name: name.to_string(),
            args: Vec::new(),
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
                generics: Generics::default(),
                operations: Vec::new(),
            }),
            name: "IO".to_string(),
            args: vec![],
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

    #[test]
    fn continuations_are_displayed_with_their_row() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut con = |name: &str| Type::Con {
            id: types.alloc(TypeDef::builtin(name)),
            name: name.to_string(),
            args: Vec::new(),
        };
        let int = con("Int");
        let unit = Type::unit();
        let io = EffectLabel {
            id: effects.alloc(EffectDef {
                name: "IO".to_string(),
                generics: Generics::default(),
                operations: Vec::new(),
            }),
            name: "IO".to_string(),
            args: vec![],
        };
        let k = Type::Cont {
            arg: Box::new(int.clone()),
            ret: Box::new(unit.clone()),
            effects: vec![io],
            tail: None,
            linearity: Linearity::Lin,
        };
        assert_eq!(k.to_string(), "Cont Int Unit <IO>");
        let pure = Type::Cont {
            arg: Box::new(Type::Fn {
                param: Box::new(int.clone()),
                linearity: Linearity::Unr,
                effects: vec![],
                tail: None,
                ret: Box::new(int),
            }),
            ret: Box::new(unit),
            effects: vec![],
            tail: Some(RowTail::Rigid("e".to_string())),
            linearity: Linearity::Lin,
        };
        assert_eq!(pure.to_string(), "Cont (Int -> Int) Unit <e>");
    }

    #[test]
    fn effect_labels_are_displayed_with_their_type_arguments() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let int = Type::Con {
            id: types.alloc(TypeDef::builtin("Int")),
            name: "Int".to_string(),
            args: Vec::new(),
        };
        let id = effects.alloc(EffectDef {
            name: "State".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        });
        let function = Type::Fn {
            param: Box::new(int.clone()),
            linearity: Linearity::Unr,
            effects: vec![],
            tail: None,
            ret: Box::new(int.clone()),
        };
        let ty = Type::Fn {
            param: Box::new(Type::unit()),
            linearity: Linearity::Unr,
            effects: vec![
                EffectLabel {
                    id,
                    name: "State".to_string(),
                    args: vec![int.clone()],
                },
                EffectLabel {
                    id,
                    name: "State".to_string(),
                    args: vec![function],
                },
            ],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: Box::new(int),
        };
        assert_eq!(
            ty.to_string(),
            "Unit -> <State Int, State (Int -> Int) | e> Int"
        );
    }
}

/// Kind の制約の片側。`Unr` と `Lin` は定数で、`Of` はその型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindTerm {
    Unr,
    Lin,
    Of(Type),
}

/// スキームに残った Kind の制約。テストの表示で使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindConstraint {
    /// `lower <= upper`。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が `Lin` なら `row` は `Once` 以下である (docs/spec/types.md の「推論」)。
    Carry { value: KindTerm, row: RowTerm },
}

/// 持ち越しの制約の row の側。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTerm {
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
        match self {
            KindConstraint::Linearity { lower, upper } => write!(f, "{lower} <= {upper}"),
            KindConstraint::Carry {
                value: KindTerm::Lin,
                row,
            } => write!(f, "{row} <= Once"),
            KindConstraint::Carry { value, row } => write!(f, "{value} => {row} <= Once"),
        }
    }
}
