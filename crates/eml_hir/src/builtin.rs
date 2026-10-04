//! 組み込みの値と関数の名前と、演算子の表。S2 で `Prelude` モジュールに移すまで、名前解決の最も外側のスコープとして扱う。

/// 組み込みの値と関数。型は Prelude (`prelude.em`) が、Core IR への変換は `eml_core_ir` が与える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Println,
    ShowInt,
    Not,
    True,
    False,
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntNeg,
    IntEq,
    IntNe,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
    ComposeFwd,
    ComposeBwd,
}

/// 名前空間での見え方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// 名前で引く値。
    Named,
    /// 二項演算子として引く。
    Operator,
    /// 名前で引けない。前置の `-` は `negate` に脱糖するが、ユーザーは `negate` と書けない。
    Internal,
}

#[derive(Debug)]
pub struct BuiltinInfo {
    pub builtin: Builtin,
    /// Prelude (`prelude.em`) と診断での書き方。
    pub name: &'static str,
    pub access: Access,
    /// 実装が受け取る引数の数。部分適用のクロージャの Kind に使う (docs/spec/types.md の「関数型」)。
    pub arity: usize,
}

/// 組み込みの表。名前と見え方と引数の数はここだけに置き、型は Prelude に置く。
pub const BUILTINS: &[BuiltinInfo] = &[
    info(Builtin::Println, "println", Access::Named, 1),
    info(Builtin::ShowInt, "show_int", Access::Named, 1),
    info(Builtin::Not, "not", Access::Named, 1),
    info(Builtin::True, "True", Access::Named, 0),
    info(Builtin::False, "False", Access::Named, 0),
    info(Builtin::IntNeg, "negate", Access::Internal, 1),
    info(Builtin::IntAdd, "+", Access::Operator, 2),
    info(Builtin::IntSub, "-", Access::Operator, 2),
    info(Builtin::IntMul, "*", Access::Operator, 2),
    info(Builtin::IntDiv, "/", Access::Operator, 2),
    info(Builtin::IntMod, "%", Access::Operator, 2),
    info(Builtin::IntEq, "==", Access::Operator, 2),
    info(Builtin::IntNe, "!=", Access::Operator, 2),
    info(Builtin::IntLt, "<", Access::Operator, 2),
    info(Builtin::IntLe, "<=", Access::Operator, 2),
    info(Builtin::IntGt, ">", Access::Operator, 2),
    info(Builtin::IntGe, ">=", Access::Operator, 2),
    info(Builtin::StrConcat, "++", Access::Operator, 2),
    info(Builtin::ComposeFwd, ">>", Access::Operator, 3),
    info(Builtin::ComposeBwd, "<<", Access::Operator, 3),
];

const fn info(builtin: Builtin, name: &'static str, access: Access, arity: usize) -> BuiltinInfo {
    BuiltinInfo {
        builtin,
        name,
        access,
        arity,
    }
}

impl Builtin {
    /// 組み込みの `IO` の操作。handler の節に書けないことを報告するのに使う (docs/spec/effects.md)。段階5で
    /// `open` などを足す。
    pub fn is_io_operation(self) -> bool {
        matches!(self, Builtin::Println)
    }

    pub fn info(self) -> &'static BuiltinInfo {
        BUILTINS
            .iter()
            .find(|info| info.builtin == self)
            .expect("every builtin has a row in the table")
    }

    /// 名前で引ける組み込み。演算子は `binary_operator` で引く。
    pub fn from_name(name: &str) -> Option<Builtin> {
        Self::find(name, |access| access == Access::Named)
    }

    /// 二項演算子の組み込み。`&&`、`||`、`|>`、`<|` は HIR で脱糖するので含まない (docs/spec/declarations.md)。
    pub fn binary_operator(op: &str) -> Option<Builtin> {
        Self::find(op, |access| access == Access::Operator)
    }

    /// Prelude のシグネチャの名前。見え方によらずに引く。
    pub fn from_prelude_name(name: &str) -> Option<Builtin> {
        Self::find(name, |_| true)
    }

    /// ソースでの書き方。診断と表示で使う。
    pub fn name(self) -> &'static str {
        self.info().name
    }

    pub fn arity(self) -> usize {
        self.info().arity
    }

    fn find(name: &str, access: impl Fn(Access) -> bool) -> Option<Builtin> {
        BUILTINS
            .iter()
            .find(|info| info.name == name && access(info.access))
            .map(|info| info.builtin)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

/// 標準の演算子の表 (docs/spec/declarations.md)。fixity の宣言は段階6で読む。
pub fn fixity(op: &str) -> Option<(u8, Assoc)> {
    Some(match op {
        "<|" => (0, Assoc::Right),
        "|>" => (1, Assoc::Left),
        "||" => (2, Assoc::Right),
        "&&" => (3, Assoc::Right),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => (4, Assoc::None),
        "++" | "::" => (5, Assoc::Right),
        "+" | "-" => (6, Assoc::Left),
        "*" | "/" | "%" => (7, Assoc::Left),
        ">>" | "<<" => (9, Assoc::Right),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_one_row_in_the_table() {
        for info in BUILTINS {
            assert_eq!(info.builtin.info().name, info.name);
            assert_eq!(
                BUILTINS
                    .iter()
                    .filter(|other| other.builtin == info.builtin)
                    .count(),
                1,
                "{}",
                info.name
            );
        }
    }

    #[test]
    fn names_are_looked_up_by_access() {
        assert_eq!(Builtin::from_name("println"), Some(Builtin::Println));
        assert_eq!(Builtin::from_name("True"), Some(Builtin::True));
        assert_eq!(Builtin::from_name("+"), None);
        assert_eq!(Builtin::from_name("negate"), None);
        assert_eq!(Builtin::binary_operator("+"), Some(Builtin::IntAdd));
        assert_eq!(Builtin::binary_operator("println"), None);
        assert_eq!(Builtin::from_prelude_name("negate"), Some(Builtin::IntNeg));
        assert_eq!(Builtin::IntNeg.name(), "negate");
        assert_eq!(Builtin::ComposeFwd.arity(), 3);
    }
}
