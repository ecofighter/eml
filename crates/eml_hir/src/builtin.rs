//! 組み込みの名前と型と演算子。S2 で `Prelude` モジュールに移すまで、名前解決の最も外側のスコープとして扱う。

/// 組み込みの値と関数。型は `eml_types` が、実装は `eml_interp` が、この enum の `match` で与える。
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

impl Builtin {
    /// 名前で引ける組み込み。演算子は `binary_operator` で引く。
    pub fn from_name(name: &str) -> Option<Builtin> {
        Some(match name {
            "println" => Builtin::Println,
            "show_int" => Builtin::ShowInt,
            "not" => Builtin::Not,
            "True" => Builtin::True,
            "False" => Builtin::False,
            _ => return None,
        })
    }

    /// 二項演算子の組み込み。`&&`、`||`、`|>`、`<|` は HIR で脱糖するので含まない (docs/spec/declarations.md)。
    pub fn binary_operator(op: &str) -> Option<Builtin> {
        Some(match op {
            "+" => Builtin::IntAdd,
            "-" => Builtin::IntSub,
            "*" => Builtin::IntMul,
            "/" => Builtin::IntDiv,
            "%" => Builtin::IntMod,
            "==" => Builtin::IntEq,
            "!=" => Builtin::IntNe,
            "<" => Builtin::IntLt,
            "<=" => Builtin::IntLe,
            ">" => Builtin::IntGt,
            ">=" => Builtin::IntGe,
            "++" => Builtin::StrConcat,
            ">>" => Builtin::ComposeFwd,
            "<<" => Builtin::ComposeBwd,
            _ => return None,
        })
    }

    /// ソースでの書き方。診断と表示で使う。
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Println => "println",
            Builtin::ShowInt => "show_int",
            Builtin::Not => "not",
            Builtin::True => "True",
            Builtin::False => "False",
            Builtin::IntAdd => "+",
            Builtin::IntSub => "-",
            Builtin::IntMul => "*",
            Builtin::IntDiv => "/",
            Builtin::IntMod => "%",
            Builtin::IntNeg => "negate",
            Builtin::IntEq => "==",
            Builtin::IntNe => "!=",
            Builtin::IntLt => "<",
            Builtin::IntLe => "<=",
            Builtin::IntGt => ">",
            Builtin::IntGe => ">=",
            Builtin::StrConcat => "++",
            Builtin::ComposeFwd => ">>",
            Builtin::ComposeBwd => "<<",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinType {
    Int,
    String,
    Bool,
    Unit,
}

impl BuiltinType {
    pub fn from_name(name: &str) -> Option<BuiltinType> {
        Some(match name {
            "Int" => BuiltinType::Int,
            "String" => BuiltinType::String,
            "Bool" => BuiltinType::Bool,
            "Unit" => BuiltinType::Unit,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            BuiltinType::Int => "Int",
            BuiltinType::String => "String",
            BuiltinType::Bool => "Bool",
            BuiltinType::Unit => "Unit",
        }
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
