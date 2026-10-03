use std::fmt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    Io,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Io => "IO",
        }
    }

    /// `IO` は実行時が必ず1回再開するので、`once` と同じ扱いになる (docs/spec/effects.md)。
    pub(crate) fn multiplicity(self) -> Multiplicity {
        match self {
            Effect::Io => Multiplicity::Once,
        }
    }
}

/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、残った変数は `Error` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    String,
    Bool,
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, Type)>),
    Fn {
        param: Box<Type>,
        linearity: Linearity,
        effects: Vec<Effect>,
        ret: Box<Type>,
    },
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
            Type::Fn { param, ret, .. } => param.contains_error() || ret.contains_error(),
            Type::Int | Type::String | Type::Bool => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => f.write_str("Int"),
            Type::String => f.write_str("String"),
            Type::Bool => f.write_str("Bool"),
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
                ret,
                ..
            } => {
                if matches!(**param, Type::Fn { .. }) {
                    write!(f, "({param}) -> ")?;
                } else {
                    write!(f, "{param} -> ")?;
                }
                // 空の row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                if !effects.is_empty() {
                    let names: Vec<&str> = effects.iter().map(|e| e.name()).collect();
                    write!(f, "<{}> ", names.join(", "))?;
                }
                write!(f, "{ret}")
            }
            Type::Error => f.write_str("{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let pure = Type::Fn {
            param: Box::new(Type::Int),
            linearity: Linearity::Unr,
            effects: vec![],
            ret: Box::new(Type::Bool),
        };
        let io = Type::Fn {
            param: Box::new(pure.clone()),
            linearity: Linearity::Unr,
            effects: vec![Effect::Io],
            ret: Box::new(Type::unit()),
        };
        assert_eq!(pure.to_string(), "Int -> Bool");
        assert_eq!(io.to_string(), "(Int -> Bool) -> <IO> Unit");
    }
}
